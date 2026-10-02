@group(0) @binding(0)
var source: texture_2d<f32>;
@group(0) @binding(1)
var source_sampler: sampler;

struct LayerUniforms {
    opacity: f32,
    filter_b: f32,
    filter_s: f32,
    filter_c: f32,
    clip_rect: vec4<f32>,
    clip_inv_abcd: vec4<f32>,
    clip_inv_ef: vec2<f32>,
    clip_corner_radius: f32,
    clip_polygon_count: u32,
    clip_poly0: vec4<f32>,
    clip_poly1: vec4<f32>,
    clip_poly2: vec4<f32>,
    clip_poly3: vec4<f32>,
    filter_hue: f32,
    filter_blur: f32,
    mix_blend: u32,
    _pad_blend: u32,
    drop_shadow_offset: vec2<f32>,
    drop_shadow_blur: f32,
    filter_invert: f32,
    drop_shadow_color: vec4<f32>,
}

@group(0) @binding(2)
var<uniform> layer: LayerUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.uv = positions[index] * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return output;
}

fn element_filter_blur(uv: vec2<f32>, radius: f32) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(source));
    let texel = 1.0 / max(dims, vec2<f32>(1.0));
    let step = radius / 2.0;
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            let d = vec2<f32>(f32(x), f32(y));
            let w = exp(-dot(d, d) * 0.5);
            acc += textureSample(source, source_sampler, uv + d * step * texel) * w;
            wsum += w;
        }
    }
    return acc / max(wsum, 0.0001);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return layer_source(input);
}

// The group's premultiplied colour at this pixel, after its clip, blur, drop
// shadow, opacity and colour filter.
fn layer_source(input: VertexOutput) -> vec4<f32> {
    // Clip inverse is stored in dest pixels (`FragmentClip::for_physical_pixels`).
    let clip_cover = fragment_clip_coverage(
        input.position.xy,
        layer.clip_rect,
        layer.clip_inv_abcd,
        layer.clip_inv_ef,
        layer.clip_corner_radius,
        layer.clip_polygon_count,
        layer.clip_poly0,
        layer.clip_poly1,
        layer.clip_poly2,
        layer.clip_poly3,
        1.0,
    );
    if clip_cover <= 0.0 {
        discard;
    }
    var sampled = textureSample(source, source_sampler, input.uv);
    if (layer.filter_blur > 0.5) {
        sampled = element_filter_blur(input.uv, layer.filter_blur);
    }
    if (layer.drop_shadow_color.a > 0.001) {
        let dims = vec2<f32>(textureDimensions(source));
        let offset_uv = layer.drop_shadow_offset / max(dims, vec2<f32>(1.0));
        let shadow_uv = input.uv - offset_uv;
        var shadow_sample = textureSample(source, source_sampler, shadow_uv);
        if (layer.drop_shadow_blur > 0.5) {
            shadow_sample = element_filter_blur(shadow_uv, layer.drop_shadow_blur);
        }
        let shadow_a = shadow_sample.a * layer.drop_shadow_color.a;
        let shadow = vec4<f32>(layer.drop_shadow_color.rgb * shadow_a, shadow_a);
        sampled = sampled + shadow * (1.0 - sampled.a);
    }
    sampled = sampled * layer.opacity;
    if (layer.filter_b != 1.0
        || layer.filter_s != 1.0
        || layer.filter_c != 1.0
        || abs(layer.filter_hue) > 0.0001
        || layer.filter_invert > 0.0001)
    {
        sampled = apply_color_filter_channels(
            sampled,
            layer.filter_b,
            layer.filter_s,
            layer.filter_c,
            layer.filter_hue,
            layer.filter_invert,
            1.0,
        );
    }
    // After the filter, which must not see the faded edge.
    return sampled * clip_cover;
}
