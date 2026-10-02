// Composite blurred dest sample with rounded rect, mask, clip, saturate.
// Clip helpers come from color.wgsl; QuadPaintData / mask_alpha from quad_paint_data.wgsl.

@group(0) @binding(0)
var blurred: texture_2d<f32>;
@group(0) @binding(1)
var blurred_sampler: sampler;

struct CompositeUniforms {
    quad_origin: vec2<f32>,
    quad_size: vec2<f32>,
    corner_radius: vec4<f32>,
    padded_origin: vec2<f32>,
    padded_size: vec2<f32>,
    dest_size: vec2<f32>,
    saturate: f32,
    clip_corner_radius: f32,
    clip_rect: vec4<f32>,
    clip_inv_abcd: vec4<f32>,
    clip_inv_ef: vec2<f32>,
    clip_polygon_count: u32,
    _pad_poly: u32,
    clip_poly0: vec4<f32>,
    clip_poly1: vec4<f32>,
    clip_poly2: vec4<f32>,
    clip_poly3: vec4<f32>,
    quad_logical_origin: vec2<f32>,
    quad_logical_size: vec2<f32>,
    quad_abcd: vec4<f32>,
    quad_ef: vec2<f32>,
    paint_index: u32,
    _pad_end: u32,
}

@group(0) @binding(2)
var<uniform> composite: CompositeUniforms;

struct PaintBuffer {
    items: array<QuadPaintData>,
}

@group(0) @binding(3)
var<storage, read> paint_buffer: PaintBuffer;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) world: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(0.0, 1.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 1.0),
    );
    var output: VertexOutput;
    let uv = positions[index];
    let m = composite.quad_abcd;
    let axis_aligned = m.x == 1.0 && m.y == 0.0 && m.z == 0.0 && m.w == 1.0;
    let size = select(composite.quad_logical_size, composite.quad_size, axis_aligned);
    // Device px (local) the quad reaches past each edge so the rounded edge's
    // outer half device pixel is rasterized under the transform, as
    // `edge_grow` in `quad_solid.wgsl`.
    let det = abs(m.x * m.w - m.y * m.z);
    let grow = min(0.5 * vec2(length(m.zw), length(m.xy)) / max(det, 1.0e-6), vec2(256.0));
    let local = uv * size + (uv * 2.0 - 1.0) * grow;
    let logical = composite.quad_logical_origin + local;
    let transformed = vec2(
        m.x * logical.x + m.z * logical.y + composite.quad_ef.x,
        m.y * logical.x + m.w * logical.y + composite.quad_ef.y,
    );
    let world = select(transformed, composite.quad_origin + local, axis_aligned);
    let ndc = vec2(
        world.x / composite.dest_size.x * 2.0 - 1.0,
        1.0 - world.y / composite.dest_size.y * 2.0,
    );
    output.position = vec4(ndc, 0.0, 1.0);
    output.local = local;
    output.world = world;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Local px per device pixel, for the rounded edge's ramp.
    let local_dx = dpdx(input.local);
    let local_dy = dpdy(input.local);
    let clip_cover = fragment_clip_coverage(
        input.world,
        composite.clip_rect,
        composite.clip_inv_abcd,
        composite.clip_inv_ef,
        composite.clip_corner_radius,
        composite.clip_polygon_count,
        composite.clip_poly0,
        composite.clip_poly1,
        composite.clip_poly2,
        composite.clip_poly3,
        1.0,
    );
    if clip_cover <= 0.0 {
        discard;
    }

    let axis_aligned = composite.quad_abcd.x == 1.0
        && composite.quad_abcd.y == 0.0
        && composite.quad_abcd.z == 0.0
        && composite.quad_abcd.w == 1.0;
    let quad_size = select(composite.quad_logical_size, composite.quad_size, axis_aligned);
    let local_uv = input.local / max(quad_size, vec2(0.0001));
    let paint = paint_buffer.items[composite.paint_index];

    let blur_uv = input.world / composite.dest_size;
    var color = textureSample(blurred, blurred_sampler, blur_uv);

    if (composite.saturate != 1.0) {
        let lum = dot(color.xyz, vec3(0.2126, 0.7152, 0.0722));
        color = vec4(mix(vec3(lum), color.xyz, composite.saturate), color.a);
    }

    let half_size = quad_size * 0.5;
    let edge = rounded_box_distance(
        input.local - half_size,
        half_size,
        composite.corner_radius,
        local_dx,
        local_dy,
    );
    var alpha = clamp(0.5 - edge, 0.0, 1.0) * clip_cover;

    if ((paint.flags & PAINT_MASK) != 0u && (paint.flags & PAINT_MASK_URL) == 0u) {
        alpha *= mask_alpha(local_uv, paint);
    }

    return vec4(color.xyz * alpha, alpha);
}
