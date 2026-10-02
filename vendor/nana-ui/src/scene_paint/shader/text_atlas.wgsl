// Shared by the text program: the projection, the two atlas pages, the two
// samplers, the presentation tables and the sRGB unpack. Keeping it in one
// file is what makes "an upright label and a rotated one sample the same page"
// true by construction rather than by two declarations agreeing.
struct Globals {
    transform: mat4x4<f32>,
    // DirectWrite's alpha correction coefficients for the platform gamma.
    gamma_ratios: vec4<f32>,
    // x: grayscale enhanced contrast, y: ClearType enhanced contrast.
    contrast: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> globals: Globals;

// One glyph's presentation. Everything here is *how* a resolved paragraph
// reaches the screen, never which glyphs it is made of, so an animation that
// only moves, fades or recolors text patches this table and leaves every
// instance alone.
struct TextRun {
    // Whole-pixel physical origin the instances are relative to.
    origin: vec2<f32>,
    // Index into `text_presentations`.
    presentation: u32,
    flags: u32,
    // Linear RGB with its own alpha, opacity not yet applied.
    color: vec4<f32>,
    opacity: f32,
    // Physical px per logical px the instances were resolved at: the device
    // scale, times the raster step a magnifying transform earned the entry.
    raster: f32,
    pad0: f32,
    pad1: f32,
}

// The transform and clip a run paints under. Deduplicated: a shell's labels
// nearly all share one identity transform and one clip, so this table stays a
// handful of entries however many paragraphs the frame holds.
struct TextPresentation {
    // `matrix(a, b, c, d, e, f)` as (a, b, c, d) and (e, f, g, h) with (g, h)
    // the projective row.
    affine: vec4<f32>,
    project: vec4<f32>,
    clip_rect: vec4<f32>,
    clip_inv_abcd: vec4<f32>,
    // (x, y) inverse translation, z corner radius, w the scene scale.
    clip_inv_ef: vec4<f32>,
    // `clip-path: polygon(...)` vertices, two per vec4.
    polygon: array<vec4<f32>, 4>,
    polygon_count: u32,
    pad0: u32,
    // Whole physical px of the translation a translated run's origin is
    // relative to, so a whole-pixel scroll rewrites this row and no run.
    translate: vec2<f32>,
}

// Sampling is bilinear rather than nearest: the quad no longer lands on the
// texel grid.
const RUN_LINEAR: u32 = 1u;
// The run carries a clip the scissor cannot express.
const RUN_CLIP: u32 = 2u;
// The run's presentation is more than a translation, so each corner goes
// through the homography.
const RUN_PROJECT: u32 = 4u;

// What a fragment samples, as the vertex stage hands it over.
const CONTENT_MASK: u32 = 0u;
const CONTENT_COLOR: u32 = 1u;
const CONTENT_SUBPIXEL: u32 = 2u;

@group(0) @binding(1)
var<storage, read> text_runs: array<TextRun>;

@group(0) @binding(2)
var<storage, read> text_presentations: array<TextPresentation>;

@group(1) @binding(0)
var mask_atlas: texture_2d<f32>;

@group(1) @binding(1)
var color_atlas: texture_2d<f32>;

@group(1) @binding(2)
var atlas_nearest: sampler;

@group(1) @binding(3)
var atlas_linear: sampler;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear3(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb3(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

// Coverage → the alpha a linear blend needs to show what DirectWrite's
// gamma-space blend would, per channel. See `text/gamma.rs`, which states the
// grayscale case on the CPU.
//
// `fg` is the sRGB-encoded foreground; `f` the brightness DirectWrite corrects
// against; `src` what each channel blends toward, whose opposite is the
// background assumed. Enhance contrast and alpha correction are ported from
// Windows Terminal's AtlasEngine `dwrite.hlsl`, Copyright (c) Microsoft
// Corporation, MIT.
fn corrected_coverage(
    coverage: vec3<f32>,
    fg: vec3<f32>,
    f: vec3<f32>,
    src: vec3<f32>,
    contrast: f32,
) -> vec3<f32> {
    let k = contrast * saturate(4.0 * (0.75 - dot(fg, vec3<f32>(0.30, 0.59, 0.11))));
    let g = globals.gamma_ratios;
    var a = coverage * (k + 1.0) / (coverage * k + 1.0);
    a = a + a * (1.0 - a) * ((g.x * f + g.y) * a + (g.z * f + g.w));
    let dst = vec3<f32>(1.0) - src;
    let low = srgb_to_linear3(dst);
    let span = srgb_to_linear3(src) - low;
    let flat = abs(src - dst) < vec3<f32>(1.0 / 256.0);
    let shown = srgb_to_linear3(mix(dst, src, a));
    return select(saturate((shown - low) / select(span, vec3<f32>(1.0), flat)), a, flat);
}

// Scene colors are sRGB-encoded and the target is linear, so the conversion is
// the same one every other pipeline applies. Doing it here rather than on the
// CPU is what lets an instance carry four bytes of color.
fn unpack_srgb(color: u32) -> vec4<f32> {
    return vec4<f32>(
        srgb_to_linear(f32((color & 0x00ff0000u) >> 16u) / 255.0),
        srgb_to_linear(f32((color & 0x0000ff00u) >> 8u) / 255.0),
        srgb_to_linear(f32(color & 0x000000ffu) / 255.0),
        f32((color & 0xff000000u) >> 24u) / 255.0,
    );
}

// The page a glyph came from, in normalized coordinates.
fn atlas_uv(texel: vec2<u32>, content: u32) -> vec2<f32> {
    var dim = vec2<u32>(1u);
    if content == 0u {
        dim = textureDimensions(mask_atlas);
    } else {
        dim = textureDimensions(color_atlas);
    }
    return vec2<f32>(texel) / vec2<f32>(dim);
}

// A glyph corner in physical paint space.
//
// The same homography `Quad` applies, in logical space, per corner. A pure
// translation skips it: the presentation row carries the translation's whole
// pixels, the run origin the paragraph's own whole pixels relative to that,
// and the instance the sub-pixel remainder its bitmap was rasterized for, so
// touching the corner at all would only round it again.
//
// A projected run's instances are in the run's raster px, which a magnified
// entry makes finer than device px, so they are taken back to logical space
// by the raster scale and out again by the device scale.
fn text_world_position(run: TextRun, local: vec2<f32>) -> vec2<f32> {
    let paint = local + run.origin;
    let presentation = text_presentations[run.presentation];
    if (run.flags & RUN_PROJECT) == 0u {
        return paint + presentation.translate;
    }
    let scale = presentation.clip_inv_ef.w;
    let p = paint / run.raster;
    let xp = presentation.affine.x * p.x + presentation.affine.z * p.y + presentation.project.x;
    let yp = presentation.affine.y * p.x + presentation.affine.w * p.y + presentation.project.y;
    let w = presentation.project.z * p.x + presentation.project.w * p.y + 1.0;
    var projected = vec2<f32>(xp, yp);
    if abs(w) >= 1e-8 {
        projected = projected / w;
    }
    return projected * scale;
}

// Raster px a projected glyph's quad reaches past each edge of its bitmap,
// so the half device pixel its filtered edge spreads over is rasterized:
// `edge_grow` in `quad_solid.wgsl`, taken to raster px at `local`. The taps
// out there read the glyph's transparent gutter.
fn text_edge_grow(run: TextRun, local: vec2<f32>) -> vec2<f32> {
    let presentation = text_presentations[run.presentation];
    let a = presentation.affine;
    let e = presentation.project;
    let p = (local + run.origin) / run.raster;
    let w = e.z * p.x + e.w * p.y + 1.0;
    let world = vec2(a.x * p.x + a.z * p.y + e.x, a.y * p.x + a.w * p.y + e.y)
        / select(1.0, w, abs(w) >= 1e-8);
    let along_x = vec2(a.x - world.x * e.z, a.y - world.y * e.z);
    let along_y = vec2(a.z - world.x * e.w, a.w - world.y * e.w);
    let det = abs(along_x.x * along_y.y - along_x.y * along_y.x);
    let per_device_px = run.raster / max(presentation.clip_inv_ef.w, 1.0e-6);
    let grow = 0.5 * abs(w) * vec2(length(along_y), length(along_x)) / max(det, 1.0e-6);
    return min(grow * per_device_px, vec2(256.0));
}

// The homography's `w` at `local`, as a projected glyph's clip-space w: its
// atlas coordinates then interpolate perspective-correctly.
fn text_clip_w(run: TextRun, local: vec2<f32>) -> f32 {
    if (run.flags & RUN_PROJECT) == 0u {
        return 1.0;
    }
    let e = text_presentations[run.presentation].project;
    let p = (local + run.origin) / run.raster;
    let w = e.z * p.x + e.w * p.y + 1.0;
    return select(1.0, w, w > 1.0e-6);
}
