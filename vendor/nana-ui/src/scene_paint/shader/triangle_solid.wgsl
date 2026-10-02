// Articulated-line stroke: one instance per segment, covering quad in the
// vertex shader, vanilla-disc SDF in local space (ellipse under non-uniform
// affine). Independent WGSL. `STROKE_AA_FRINGE` is the physical-pixel pad;
// identity + viewport 1 keeps 1 logical px (GraphCanvas 1.6px covering).
const STROKE_AA_FRINGE: f32 = 1.0;

// Unique clips interned on the CPU; instances store an index.
// `inv_ef_radius.w` is polygon vertex count; `poly0..3` pack ≤8 clip-path
// vertices in rect-local space (same as dest `FragmentClip`).
struct GpuClip {
    rect: vec4<f32>,
    inv_abcd: vec4<f32>,
    inv_ef_radius: vec4<f32>,
    poly0: vec4<f32>,
    poly1: vec4<f32>,
    poly2: vec4<f32>,
    poly3: vec4<f32>,
}

struct ClipPalette {
    items: array<GpuClip>,
}

@group(0) @binding(1)
var<storage, read> clip_palette: ClipPalette;

struct SolidInstanceInput {
    @location(0) color: vec4<f32>,
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) radii: vec2<f32>,
    @location(4) clip_index: u32,
    @location(5) packed_caps: f32,
    @location(6) affine_abcd: vec4<f32>,
    @location(7) affine_ef: vec2<f32>,
}

struct SolidVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) @interpolate(flat) clip_index: u32,
    @location(3) @interpolate(flat) p0: vec2<f32>,
    @location(4) @interpolate(flat) p1: vec2<f32>,
    @location(5) @interpolate(flat) radii_caps: vec4<f32>,
    @location(6) @interpolate(flat) affine_abcd: vec4<f32>,
    @location(7) @interpolate(flat) affine_ef: vec2<f32>,
    // `globals.viewport_scale`: the uniform is bound to the vertex stage only.
    @location(8) @interpolate(flat) pixel_scale: f32,
}

fn unit_corner(vertex_index: u32) -> vec2<f32> {
    let id = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u)[vertex_index];
    return vec2<f32>(
        select(-1.0, 1.0, (id & 2u) != 0u),
        select(-1.0, 1.0, id == 1u || id == 2u),
    );
}

fn unpack_cap0(packed: f32) -> f32 {
    return select(0.0, 1.0, packed == 1.0 || packed == 3.0);
}

fn unpack_cap1(packed: f32) -> f32 {
    return select(0.0, 1.0, packed >= 2.0);
}

// External-tangent hull half-width. Nested discs use the larger pad.
fn hull_half_width(along: f32, pad0: f32, pad1: f32, seg_len: f32) -> f32 {
    let dr = pad0 - pad1;
    if abs(dr) >= seg_len {
        return max(pad0, pad1);
    }
    let sin_a = dr / seg_len;
    let cos_a = sqrt(max(1.0 - sin_a * sin_a, 0.0));
    return (pad0 - sin_a * along) / max(cos_a, 1e-8);
}

fn covering_corner(
    p0: vec2<f32>,
    p1: vec2<f32>,
    r0: f32,
    r1: f32,
    cap0: f32,
    cap1: f32,
    fringe: f32,
    corner: vec2<f32>,
) -> vec2<f32> {
    let delta = p1 - p0;
    let seg_len = max(length(delta), 1e-8);
    let tangent = delta / seg_len;
    let normal = vec2<f32>(-tangent.y, tangent.x);
    let pad0 = r0 + fringe;
    let pad1 = r1 + fringe;
    var end0 = select(pad0, fringe, cap0 > 0.5);
    var end1 = select(pad1, fringe, cap1 > 0.5);
    // Nested discs: the hull is the larger disc. Extend a round far end so
    // the covering quad still contains it; a butt cut keeps the end plane.
    if pad0 >= pad1 + seg_len && cap1 < 0.5 {
        end1 = max(end1, pad0 - seg_len);
    }
    if pad1 >= pad0 + seg_len && cap0 < 0.5 {
        end0 = max(end0, pad1 - seg_len);
    }
    let along = select(-end0, seg_len + end1, corner.x > 0.0);
    let side = hull_half_width(along, pad0, pad1, seg_len);
    return p0 + tangent * along + normal * (corner.y * side);
}

// CSS/Canvas `matrix(a, b, c, d, e, f)`: x' = ax + cy + e, y' = bx + dy + f.
// `abcd` is (a, b, c, d). Identity skips the multiply.
fn is_identity_affine(abcd: vec4<f32>, ef: vec2<f32>) -> bool {
    return all(abcd == vec4<f32>(1.0, 0.0, 0.0, 1.0)) && all(ef == vec2<f32>(0.0));
}

fn apply_affine(abcd: vec4<f32>, ef: vec2<f32>, p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        abcd.x * p.x + abcd.z * p.y + ef.x,
        abcd.y * p.x + abcd.w * p.y + ef.y,
    );
}

fn local_to_world(abcd: vec4<f32>, ef: vec2<f32>, p: vec2<f32>) -> vec2<f32> {
    if is_identity_affine(abcd, ef) {
        return p;
    }
    return apply_affine(abcd, ef, p);
}

fn world_to_local(abcd: vec4<f32>, ef: vec2<f32>, p: vec2<f32>) -> vec2<f32> {
    if is_identity_affine(abcd, ef) {
        return p;
    }
    let det = abcd.x * abcd.w - abcd.y * abcd.z;
    if abs(det) < 1e-12 {
        return p;
    }
    let inv_det = 1.0 / det;
    let ia = abcd.w * inv_det;
    let ib = -abcd.y * inv_det;
    let ic = -abcd.z * inv_det;
    let id = abcd.x * inv_det;
    let ie = -(ia * ef.x + ic * ef.y);
    let i_f = -(ib * ef.x + id * ef.y);
    return vec2<f32>(ia * p.x + ic * p.y + ie, ib * p.x + id * p.y + i_f);
}

// Local pad covering `STROKE_AA_FRINGE` physical px after affine + viewport.
fn local_aa_fringe(abcd: vec4<f32>) -> f32 {
    var sigma = 1.0;
    if !all(abcd == vec4<f32>(1.0, 0.0, 0.0, 1.0)) {
        let a = abcd.x;
        let b = abcd.y;
        let c = abcd.z;
        let d = abcd.w;
        let det = a * d - b * c;
        let fro2 = a * a + b * b + c * c + d * d;
        let disc = max(fro2 * fro2 - 4.0 * det * det, 0.0);
        sigma = sqrt(max((fro2 - sqrt(disc)) * 0.5, 0.0));
    }
    let viewport = max(globals.viewport_scale, 1e-4);
    return STROKE_AA_FRINGE / max(sigma * viewport, 1e-4);
}

@vertex
fn solid_vs_main(
    @builtin(vertex_index) vertex_index: u32,
    input: SolidInstanceInput,
) -> SolidVertexOutput {
    var out: SolidVertexOutput;
    let packed = input.packed_caps;
    let cap0 = unpack_cap0(packed);
    let cap1 = unpack_cap1(packed);
    let local = covering_corner(
        input.p0,
        input.p1,
        input.radii.x,
        input.radii.y,
        cap0,
        cap1,
        local_aa_fringe(input.affine_abcd),
        unit_corner(vertex_index),
    );
    let world = local_to_world(input.affine_abcd, input.affine_ef, local);
    out.color = premultiply(input.color);
    out.position = globals.transform * vec4<f32>(world, 0.0, 1.0);
    out.world_pos = world;
    out.clip_index = input.clip_index;
    out.pixel_scale = globals.viewport_scale;
    out.p0 = input.p0;
    out.p1 = input.p1;
    out.radii_caps = vec4<f32>(input.radii, cap0, cap1);
    out.affine_abcd = input.affine_abcd;
    out.affine_ef = input.affine_ef;
    return out;
}

fn radial(v: vec2<f32>) -> vec2<f32> {
    let length_v = length(v);
    return select(vec2<f32>(1.0, 0.0), v / length_v, length_v > 1e-8);
}

// Convex hull of discs (a, r0) and (b, r1). Nested: the larger disc is the
// shape. The distance, and in `.yz` its unit gradient: the analytic normal,
// whose sign flips across the centre line without its length changing.
fn sd_variable_capsule(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, r0: f32, r1: f32) -> vec3<f32> {
    let ba = b - a;
    let l = length(ba);
    if l < 1e-8 {
        return vec3<f32>(length(p - a) - max(r0, r1), radial(p - a));
    }
    let tangent = ba / l;
    let normal = vec2<f32>(-tangent.y, tangent.x);
    let pa = p - a;
    let along = dot(pa, tangent);
    let side = select(-1.0, 1.0, dot(pa, normal) >= 0.0);
    let perp = abs(dot(pa, normal));
    let dr = r0 - r1;
    if abs(dr) >= l {
        if r0 > r1 {
            return vec3<f32>(length(p - a) - r0, radial(p - a));
        }
        return vec3<f32>(length(p - b) - r1, radial(p - b));
    }
    let sin_a = dr / l;
    let cos_a = sqrt(max(1.0 - sin_a * sin_a, 0.0));
    let k = along * cos_a - perp * sin_a;
    if k < 0.0 {
        return vec3<f32>(length(p - a) - r0, radial(p - a));
    }
    if k > cos_a * l {
        return vec3<f32>(length(p - b) - r1, radial(p - b));
    }
    return vec3<f32>(along * sin_a + perp * cos_a - r0, tangent * sin_a + normal * (side * cos_a));
}

fn stroke_signed_distance(
    p: vec2<f32>,
    p0: vec2<f32>,
    p1: vec2<f32>,
    r0: f32,
    r1: f32,
    cap0: f32,
    cap1: f32,
) -> vec3<f32> {
    var shape = sd_variable_capsule(p, p0, p1, r0, r1);
    if cap0 > 0.5 || cap1 > 0.5 {
        let ba = p1 - p0;
        let seg_len = max(length(ba), 1e-8);
        let tangent = ba / seg_len;
        let local_x = dot(p - p0, tangent);
        if cap0 > 0.5 && -local_x > shape.x {
            shape = vec3<f32>(-local_x, -tangent);
        }
        if cap1 > 0.5 && local_x - seg_len > shape.x {
            shape = vec3<f32>(local_x - seg_len, tangent);
        }
    }
    return shape;
}

// The distance to the stroke in device px, and how much of this fragment the
// clip keeps. The local distance goes to the screen across its own normal
// through the affine's inverse, not by `dpdx` of the distance, whose finite
// difference collapses across a thin stroke's centre line when both sides
// fall in one 2x2 quad.
fn stroke_clip_and_distance(input: SolidVertexOutput) -> vec2<f32> {
    let clip = clip_palette.items[input.clip_index];
    let cover = fragment_clip_coverage(
        input.world_pos,
        clip.rect,
        clip.inv_abcd,
        clip.inv_ef_radius.xy,
        clip.inv_ef_radius.z,
        u32(clip.inv_ef_radius.w),
        clip.poly0,
        clip.poly1,
        clip.poly2,
        clip.poly3,
        input.pixel_scale,
    );
    if cover <= 0.0 {
        discard;
    }
    let local_p = world_to_local(input.affine_abcd, input.affine_ef, input.world_pos);
    let shape = stroke_signed_distance(
        local_p,
        input.p0,
        input.p1,
        input.radii_caps.x,
        input.radii_caps.y,
        input.radii_caps.z,
        input.radii_caps.w,
    );
    // Local px per device pixel along each screen axis: the inverse's columns
    // over the viewport scale (world is logical px).
    let abcd = input.affine_abcd;
    let det = abcd.x * abcd.w - abcd.y * abcd.z;
    let local_per_px = select(
        vec4<f32>(1.0, 0.0, 0.0, 1.0),
        vec4<f32>(abcd.w, -abcd.y, -abcd.z, abcd.x) / det,
        abs(det) >= 1e-12,
    ) / max(input.pixel_scale, 1e-4);
    let per_pixel = length(vec2<f32>(dot(shape.yz, local_per_px.xy), dot(shape.yz, local_per_px.zw)));
    return vec2<f32>(shape.x / max(per_pixel, 1e-6), cover);
}

// One fragment entry for every sample count: WebGPU evaluates the fragment
// shader at the pixel center and broadcasts alpha to all samples, while the
// hull is padded past the visual edge, so MSAA cannot own an SDF edge — the
// analytic coverage below is the only AA this stroke gets.
@fragment
fn solid_fs_main(input: SolidVertexOutput) -> @location(0) vec4<f32> {
    let clipped = stroke_clip_and_distance(input);
    // A linear ramp over one device pixel, as every other edge.
    let alpha = clamp(0.5 - clipped.x, 0.0, 1.0) * clipped.y;
    if alpha <= 0.0 {
        discard;
    }
    return input.color * alpha;
}
