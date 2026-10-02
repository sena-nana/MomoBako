@group(0) @binding(0)
var source: texture_2d<f32>;
@group(0) @binding(1)
var source_sampler: sampler;

struct BlurUniforms {
    direction: vec2<f32>,
    radius: f32,
    _pad0: f32,
    texel_size: vec2<f32>,
    region_origin: vec2<f32>,
    region_size: vec2<f32>,
    dest_size: vec2<f32>,
}

@group(0) @binding(2)
var<uniform> blur: BlurUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
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
    output.local = positions[index] * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return output;
}

fn gaussian_weight(offset: f32, sigma: f32) -> f32 {
    return exp(-0.5 * (offset * offset) / (sigma * sigma));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let sigma = max(blur.radius * 0.333333, 0.5);
    let step = max(blur.radius * 0.333333, 1.0);
    var accum = vec4(0.0);
    var weight_sum = 0.0;
    for (var i = -3; i <= 3; i = i + 1) {
        let offset = f32(i) * step;
        let w = gaussian_weight(offset, sigma);
        let dest_uv =
            (blur.region_origin + input.local * blur.region_size) / blur.dest_size;
        let sample_uv = dest_uv + blur.direction * offset * blur.texel_size;
        accum += textureSample(source, source_sampler, sample_uv) * w;
        weight_sum += w;
    }
    return accum / max(weight_sum, 0.0001);
}
