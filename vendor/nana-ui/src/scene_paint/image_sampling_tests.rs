//! `ImageSampling`: a `url(...)` image resampled to its painted size by
//! default, or given a mip chain and sampled trilinearly on request; a
//! HostTexture node sampling its host's mip chain trilinearly on request.
use super::*;
use nana_ui_core::ImageSampling;

/// 1px black and white columns: bilinear minification of these shows every
/// skipped or half-weighted column, a correct reduction is flat mid grey.
fn stripes_data_url(width: u32, height: u32) -> String {
    use base64::Engine as _;
    let image = image::RgbaImage::from_fn(width, height, |x, _| {
        let value = if x % 2 == 0 { 0 } else { 255 };
        image::Rgba([value, value, value, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode png");
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    )
}

fn stretched_image_scene(url: &str, sampling: ImageSampling, size: f32) -> UiScene {
    let surface = nana_ui_scene::QuadSurfacePaint {
        background_image: Some(
            nana_ui_core::BackgroundImage::url_with_fit(
                url,
                nana_ui_core::BackgroundImageFit::Stretch,
            )
            .with_sampling(sampling),
        ),
        ..Default::default()
    };
    let mut scene = UiScene::new();
    scene.apply_delta(
        [paint_surface_quad_node(
            1,
            0.0,
            0.0,
            size,
            size,
            [0.0, 0.0, 0.0, 0.0],
            surface,
        )],
        [],
    );
    scene
}

fn viewport(size: u32) -> ScenePaintViewport {
    ScenePaintViewport {
        logical_size: [size as f32; 2],
        physical_size: [size; 2],
        scale_factor: 1.0,
        scene_origin: [0.0, 0.0],
        target_origin: [0.0, 0.0],
        clear_color: [0.0, 0.0, 0.0, 1.0],
        clear: true,
    }
}

/// Paint once and read the target back.
fn paint_once(
    painter: &mut SceneWgpuPainter,
    scene: &UiScene,
    size: u32,
    registry: Option<&HostTextureRegistry>,
) -> Vec<u8> {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let (texture, view) = test_copy_target(&device, format, size, size);
    let mut encoder = device.create_command_encoder(&Default::default());
    painter
        .paint_encoder(scene, &mut encoder, &view, viewport(size), registry, None)
        .unwrap();
    readback_rgba(&device, &queue, encoder, &texture, size, size)
}

/// Paint until no fetch, resample or mip job is outstanding, then read back.
fn paint_settled(painter: &mut SceneWgpuPainter, scene: &UiScene, size: u32) -> Vec<u8> {
    let deadline = Instant::now() + std::time::Duration::from_secs(20);
    let mut pixels = paint_once(painter, scene, size, None);
    while painter.has_pending_images() {
        assert!(Instant::now() < deadline, "image work did not settle");
        std::thread::sleep(std::time::Duration::from_millis(5));
        pixels = paint_once(painter, scene, size, None);
    }
    // The paint that collects a result also uploads and draws it.
    pixels
}

/// Lowest and highest red over the square's interior.
fn red_range(pixels: &[u8], size: u32, inner: std::ops::Range<u32>) -> (u8, u8) {
    let mut range = (u8::MAX, u8::MIN);
    for y in inner.clone() {
        for x in inner.clone() {
            let red = pixel(pixels, size, x, y)[0];
            range = (range.0.min(red), range.1.max(red));
        }
    }
    range
}

#[test]
fn a_url_image_is_resampled_to_its_painted_size_by_default() {
    let url = stripes_data_url(256, 256);
    let scene = stretched_image_scene(&url, ImageSampling::Resample, 40.0);
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);

    // The first paint decodes on the frame path and shows the decoded size;
    // bilinear 6.4:1 minification of 1px stripes is the aliasing this fixes.
    let first = paint_once(&mut painter, &scene, 64, None);
    assert_eq!(painter.url_cache.texture_shape(&url), Some((256, 256, 1)));
    let (low, high) = red_range(&first, 64, 4..36);
    assert!(
        high - low > 100,
        "the fixture must alias when not resampled, got {low}..{high}"
    );

    let settled = paint_settled(&mut painter, &scene, 64);
    assert_eq!(
        painter.url_cache.texture_shape(&url),
        Some((40, 40, 1)),
        "one level at the painted device-pixel size"
    );
    let (low, high) = red_range(&settled, 64, 4..36);
    assert!(
        low >= 170 && high <= 205,
        "resampled stripes must read as linear-light mid grey, got {low}..{high}"
    );

    // Drawn larger: the texture grows at once, up to the painted size.
    let larger = stretched_image_scene(&url, ImageSampling::Resample, 120.0);
    paint_settled(&mut painter, &larger, 128);
    assert_eq!(painter.url_cache.texture_shape(&url), Some((120, 120, 1)));
}

#[test]
fn a_mipmapped_url_image_keeps_its_size_and_samples_trilinearly() {
    let url = stripes_data_url(256, 256);
    let key = super::url_texture_cache::cache_key(&url, ImageSampling::Mipmap).into_owned();
    let scene = stretched_image_scene(&url, ImageSampling::Mipmap, 40.0);
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);

    paint_once(&mut painter, &scene, 64, None);
    assert_eq!(
        painter.url_cache.texture_shape(&key),
        Some((256, 256, 1)),
        "shown at its decoded size until the worker's chain arrives"
    );
    assert_eq!(
        painter.url_cache.texture_shape(&url),
        None,
        "the mip entry is not the default entry"
    );

    let settled = paint_settled(&mut painter, &scene, 64);
    assert_eq!(
        painter.url_cache.texture_shape(&key),
        Some((256, 256, 9)),
        "decoded size plus every level down to 1x1"
    );
    let (low, high) = red_range(&settled, 64, 4..36);
    assert!(
        low >= 170 && high <= 205,
        "trilinear sampling of the chain must read as mid grey, got {low}..{high}"
    );
}

/// 96px white level 0 over a 48px red level 1.
fn two_level_host_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui image sampling host mips"),
        size: wgpu::Extent3d {
            width: 96,
            height: 96,
            depth_or_array_layers: 1,
        },
        mip_level_count: 2,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip_level, size, texel) in [(0, 96, [255, 255, 255, 255]), (1, 48, [255, 0, 0, 255])] {
        let rgba: Vec<u8> = std::iter::repeat_n(texel, (size * size) as usize)
            .flatten()
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&Default::default())
}

fn host_texture_center(sampling: ImageSampling) -> [u8; 4] {
    let (device, queue) = test_device();
    let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let preview = context
        .create_component(document, GpuTextureView::new("layer").sampling(sampling))
        .unwrap();
    let mut layout = MutationQueue::new();
    write_box(&mut layout, preview.stable_id(), 0.0, 0.0, 64.0, 64.0);
    context.commit_mutations(layout).unwrap();
    let scene = commit_scene(&mut context);
    let view = two_level_host_texture(&device, &queue);
    let registry = register_host_texture("layer", &view, 96, 96);
    let pixels = paint_once(&mut painter, &scene, 64, Some(&registry));
    pixel(&pixels, 64, 32, 32)
}

#[test]
fn a_host_texture_samples_its_mip_chain_trilinearly_only_on_request() {
    // 96 → 64 is a level-of-detail of log2(1.5) ≈ 0.58.
    let default = host_texture_center(ImageSampling::Resample);
    assert!(
        default[1] < 16,
        "the default picks the nearest level (red), got {default:?}"
    );
    let mipmapped = host_texture_center(ImageSampling::Mipmap);
    assert!(
        (60..=160).contains(&mipmapped[1]) && mipmapped[0] > 240,
        "trilinear blends white level 0 into red level 1, got {mipmapped:?}"
    );
}

/// A single-level 64px texture of 1px black and white columns.
fn host_stripes(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: 64,
        height: 64,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui image sampling host stripes"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let rgba: Vec<u8> = (0..64 * 64)
        .flat_map(|index| {
            let value = if index % 2 == 0 { 0 } else { 255 };
            [value, value, value, 255]
        })
        .collect();
    queue.write_texture(
        texture.as_image_copy(),
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * 64),
            rows_per_image: Some(64),
        },
        size,
    );
    texture.create_view(&Default::default())
}

#[test]
fn a_single_level_host_texture_shown_smaller_filters_over_the_device_pixel() {
    // 64 texels into 28 px, by its box or by `scale(28 / 64)` about the box's
    // centre, and into 9 px: one bilinear tap per pixel lands anywhere between
    // a black and a white column; the pixel's whole footprint is mid grey.
    let (device, queue) = test_device();
    let view = host_stripes(&device, &queue);
    let registry = register_host_texture("layer", &view, 64, 64);
    let cases = [
        (28.0, 1.0, 3..25),
        (64.0, 28.0 / 64.0, 21..43),
        (9.0, 1.0, 2..7),
    ];
    for (side, scale, inner) in cases {
        let mut context = AppContext::new();
        let mut style = NodeStyle::default();
        Arc::make_mut(&mut style.layout).transform = scale_transform(scale);
        let preview = context
            .create_component(
                DocumentId::new(1).unwrap(),
                GpuTextureView::new("layer").style(style),
            )
            .unwrap();
        let mut layout = MutationQueue::new();
        write_box(&mut layout, preview.stable_id(), 0.0, 0.0, side, side);
        context.commit_mutations(layout).unwrap();
        let scene = commit_scene(&mut context);
        let mut painter = SceneWgpuPainter::for_test(wgpu::TextureFormat::Rgba8Unorm);
        let pixels = paint_once(&mut painter, &scene, 64, Some(&registry));
        let (low, high) = red_range(&pixels, 64, inner);
        assert!(
            low >= 90 && high <= 165,
            "stripes shown at scale {scale} in a {side} px box must read as mid grey, \
             got {low}..{high}"
        );
    }
}
