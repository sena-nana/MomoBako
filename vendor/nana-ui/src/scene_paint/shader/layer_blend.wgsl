// Dest-group composite for CSS blend modes the blend unit cannot express
// (Compositing and Blending Level 1). The parent target was copied into
// `backdrop` just before this draw; the result replaces it.

@group(0) @binding(3)
var backdrop: texture_2d<f32>;

fn blend_channel(mode: u32, cb: f32, cs: f32) -> f32 {
    switch mode {
        case 3u: { // overlay = hard-light with the layers swapped
            return select(1.0 - 2.0 * (1.0 - cs) * (1.0 - cb), 2.0 * cs * cb, cb <= 0.5);
        }
        case 4u: {
            return min(cb, cs);
        }
        case 5u: {
            return max(cb, cs);
        }
        case 6u: { // color-dodge
            if cb <= 0.0 {
                return 0.0;
            }
            if cs >= 1.0 {
                return 1.0;
            }
            return min(1.0, cb / (1.0 - cs));
        }
        case 7u: { // color-burn
            if cb >= 1.0 {
                return 1.0;
            }
            if cs <= 0.0 {
                return 0.0;
            }
            return 1.0 - min(1.0, (1.0 - cb) / cs);
        }
        case 8u: { // hard-light
            return select(1.0 - 2.0 * (1.0 - cb) * (1.0 - cs), 2.0 * cb * cs, cs <= 0.5);
        }
        case 9u: { // soft-light
            if cs <= 0.5 {
                return cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb);
            }
            let d = select(sqrt(cb), ((16.0 * cb - 12.0) * cb + 4.0) * cb, cb <= 0.25);
            return cb + (2.0 * cs - 1.0) * (d - cb);
        }
        case 10u: {
            return abs(cb - cs);
        }
        case 11u: {
            return cb + cs - 2.0 * cb * cs;
        }
        default: {
            return cs;
        }
    }
}

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(min(c.r, c.g), c.b);
    let x = max(max(c.r, c.g), c.b);
    var out = c;
    if n < 0.0 {
        out = l + (out - l) * l / max(l - n, 1e-6);
    }
    if x > 1.0 {
        out = l + (out - l) * (1.0 - l) / max(x - l, 1e-6);
    }
    return out;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + (l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(max(c.r, c.g), c.b) - min(min(c.r, c.g), c.b);
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let mx = max(max(c.r, c.g), c.b);
    let mn = min(min(c.r, c.g), c.b);
    if mx <= mn {
        return vec3<f32>(0.0);
    }
    return (c - mn) * s / (mx - mn);
}

fn blend_rgb(mode: u32, cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    switch mode {
        case 12u: {
            return set_lum(set_sat(cs, sat(cb)), lum(cb));
        }
        case 13u: {
            return set_lum(set_sat(cb, sat(cs)), lum(cb));
        }
        case 14u: {
            return set_lum(cs, lum(cb));
        }
        case 15u: {
            return set_lum(cb, lum(cs));
        }
        default: {
            return vec3<f32>(
                blend_channel(mode, cb.r, cs.r),
                blend_channel(mode, cb.g, cs.g),
                blend_channel(mode, cb.b, cs.b),
            );
        }
    }
}

@fragment
fn fs_blend(input: VertexOutput) -> @location(0) vec4<f32> {
    // Premultiplied source after the group's opacity, filter and clip.
    let src = layer_source(input);
    let dst = textureSample(backdrop, source_sampler, input.uv);
    let cs = select(vec3<f32>(0.0), src.rgb / src.a, src.a > 0.0);
    let cb = select(vec3<f32>(0.0), dst.rgb / dst.a, dst.a > 0.0);
    let mixed = clamp(blend_rgb(layer.mix_blend, cb, cs), vec3<f32>(0.0), vec3<f32>(1.0));
    let rgb = src.rgb * (1.0 - dst.a) + dst.rgb * (1.0 - src.a) + src.a * dst.a * mixed;
    let alpha = src.a + dst.a - src.a * dst.a;
    return vec4<f32>(rgb, alpha);
}
