// Adapted from historical Iced (MIT).
fn premultiply(color: vec4<f32>) -> vec4<f32> {
    return vec4(color.xyz * color.a, color.a);
}

fn clip_apply_affine(abcd: vec4<f32>, ef: vec2<f32>, p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        abcd.x * p.x + abcd.z * p.y + ef.x,
        abcd.y * p.x + abcd.w * p.y + ef.y
    );
}

fn clip_polygon_point(index: u32, poly0: vec4<f32>, poly1: vec4<f32>, poly2: vec4<f32>, poly3: vec4<f32>) -> vec2<f32> {
    switch index {
        case 0u: { return poly0.xy; }
        case 1u: { return poly0.zw; }
        case 2u: { return poly1.xy; }
        case 3u: { return poly1.zw; }
        case 4u: { return poly2.xy; }
        case 5u: { return poly2.zw; }
        case 6u: { return poly3.xy; }
        default: { return poly3.zw; }
    }
}

fn point_in_clip_polygon(local: vec2<f32>, count: u32, poly0: vec4<f32>, poly1: vec4<f32>, poly2: vec4<f32>, poly3: vec4<f32>) -> bool {
    if (count < 3u) {
        return true;
    }
    var winding = 0;
    for (var i: u32 = 0u; i < count; i = i + 1u) {
        let j = (i + 1u) % count;
        let vi = clip_polygon_point(i, poly0, poly1, poly2, poly3);
        let vj = clip_polygon_point(j, poly0, poly1, poly2, poly3);
        if (vi.y <= local.y) {
            if (vj.y > local.y) {
                let cross = (vj.x - vi.x) * (local.y - vi.y) - (local.x - vi.x) * (vj.y - vi.y);
                if (cross > 0.0) {
                    winding = winding + 1;
                }
            }
        } else if (vj.y <= local.y) {
            let cross = (vj.x - vi.x) * (local.y - vi.y) - (local.x - vi.x) * (vj.y - vi.y);
            if (cross < 0.0) {
                winding = winding - 1;
            }
        }
    }
    return winding != 0;
}

// Device px from `p` to the nearest side of the polygon in `poly0..3` (two
// points per vec4), each side taken to the screen through the Jacobian whose
// columns `dx`/`dy` are `p`'s screen derivatives: exact under any affine map.
// Unsigned: the caller's own fill rule says which side is inside.
fn polygon_edge_distance(
    p: vec2<f32>,
    count: u32,
    poly0: vec4<f32>,
    poly1: vec4<f32>,
    poly2: vec4<f32>,
    poly3: vec4<f32>,
    dx: vec2<f32>,
    dy: vec2<f32>,
) -> f32 {
    // `J⁻¹` up to its determinant, which the distance divides out at the end.
    let adjugate = mat2x2(vec2(dy.y, -dx.y), vec2(-dy.x, dx.x));
    var nearest = 3.0e38;
    for (var i: u32 = 0u; i < count; i = i + 1u) {
        let a = clip_polygon_point(i, poly0, poly1, poly2, poly3);
        let b = clip_polygon_point((i + 1u) % count, poly0, poly1, poly2, poly3);
        let side = adjugate * (b - a);
        let to_p = adjugate * (p - a);
        let t = clamp(dot(to_p, side) / max(dot(side, side), 1.0e-30), 0.0, 1.0);
        nearest = min(nearest, length(to_p - side * t));
    }
    return nearest / max(abs(dx.x * dy.y - dx.y * dy.x), 1.0e-12);
}

// How much of this fragment the clip keeps, 0 (discard) to 1. Every edge,
// rectangle, rounded, elliptical or polygonal, ramps over one device pixel
// like the quad's own edge. `pixels_per_world` is the scale factor for
// logical `world`, 1 for a clip `for_physical_pixels`.
fn fragment_clip_coverage(
    world: vec2<f32>,
    rect: vec4<f32>,
    inv_abcd: vec4<f32>,
    inv_ef: vec2<f32>,
    corner_radius: f32,
    polygon_count: u32,
    poly0: vec4<f32>,
    poly1: vec4<f32>,
    poly2: vec4<f32>,
    poly3: vec4<f32>,
    pixels_per_world: f32,
) -> f32 {
    // `FragmentClip::PASS`, what most unclipped fragments carry, keeps all.
    if (polygon_count == 0u && corner_radius <= 0.0 && all(rect.zw >= vec2(1.0e7))) {
        return 1.0;
    }
    let local = clip_apply_affine(inv_abcd, inv_ef, world);
    let rel = local - rect.xy;
    // Clip-local px per device pixel along each screen axis: world moves
    // `1 / pixels_per_world` per pixel and the inverse's columns take it to
    // clip-local space. Exact for any affine clip, and no `dpdx`, which some
    // callers could not take here.
    let dx = inv_abcd.xy / pixels_per_world;
    let dy = inv_abcd.zw / pixels_per_world;
    if (polygon_count == 1u) {
        // First order: the ellipse's implicit function over its screen gradient.
        let half = max(rect.zw * 0.5, vec2(0.0001));
        let n = (rel - half) / half;
        let len_n = length(n);
        let gradient = n / half / max(len_n, 1.0e-6);
        let d = (len_n - 1.0) / max(length(vec2(dot(gradient, dx), dot(gradient, dy))), 1.0e-6);
        return clamp(0.5 - d, 0.0, 1.0);
    }
    let half = rect.zw * 0.5;
    let radius = min(corner_radius, min(half.x, half.y));
    var cover = clamp(0.5 - rounded_box_distance(rel - half, half, vec4(radius), dx, dy), 0.0, 1.0);
    if (polygon_count >= 3u) {
        let edge = polygon_edge_distance(rel, polygon_count, poly0, poly1, poly2, poly3, dx, dy);
        let inside = point_in_clip_polygon(rel, polygon_count, poly0, poly1, poly2, poly3);
        cover = min(cover, clamp(0.5 - select(edge, -edge, inside), 0.0, 1.0));
    }
    return cover;
}

fn unpack_color(data: vec2<u32>) -> vec4<f32> {
    return premultiply(unpack_u32(data));
}

fn unpack_u32(data: vec2<u32>) -> vec4<f32> {
    let rg: vec2<f32> = unpack2x16float(data.x);
    let ba: vec2<f32> = unpack2x16float(data.y);

    return vec4<f32>(rg.y, rg.x, ba.y, ba.x);
}

fn apply_hue_rotate(rgb: vec3<f32>, deg: f32) -> vec3<f32> {
    if (abs(deg) < 0.0001) {
        return rgb;
    }
    let rad = deg * 0.01745329252;
    let c = cos(rad);
    let s = sin(rad);
    let col0 = vec3<f32>(
        0.213 + 0.787 * c - 0.213 * s,
        0.213 - 0.213 * c + 0.143 * s,
        0.213 - 0.213 * c - 0.787 * s,
    );
    let col1 = vec3<f32>(
        0.715 - 0.715 * c - 0.715 * s,
        0.715 + 0.285 * c + 0.140 * s,
        0.715 - 0.715 * c + 0.715 * s,
    );
    let col2 = vec3<f32>(
        0.072 - 0.072 * c + 0.928 * s,
        0.072 - 0.072 * c - 0.283 * s,
        0.072 + 0.928 * c + 0.072 * s,
    );
    return clamp(mat3x3<f32>(col0, col1, col2) * rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn apply_color_filter_channels(
    color: vec4<f32>,
    brightness: f32,
    saturate: f32,
    contrast: f32,
    hue_deg: f32,
    invert: f32,
    opacity: f32,
) -> vec4<f32> {
    var rgb = color.xyz * brightness;
    let lum = dot(rgb, vec3(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3(lum), rgb, saturate);
    rgb = (rgb - 0.5) * contrast + 0.5;
    rgb = apply_hue_rotate(clamp(rgb, vec3(0.0), vec3(1.0)), hue_deg);
    rgb = mix(rgb, vec3(1.0) - rgb, invert);
    return vec4(clamp(rgb, vec3(0.0), vec3(1.0)), color.a * opacity);
}

// Signed distance in device px from `p` to a rounded box of half extents
// `half` and per-corner `radii` (in `rounded_box_sdf`'s order), all in the
// local px `p` is measured in; `dx`/`dy` are that position's screen
// derivatives. Each side is measured across its own normal and the nearer one
// on screen wins, which a distance taken in local px and scaled afterwards
// gets wrong under an anisotropic transform; past a square corner the corner
// is the nearest point. Both exact under any affine map, a rounded corner's
// arc to first order along its normal.
fn rounded_box_distance(p: vec2<f32>, half: vec2<f32>, radii: vec4<f32>, dx: vec2<f32>, dy: vec2<f32>) -> f32 {
    let pair = select(radii.yz, radii.xw, p.x > 0.0);
    let radius = select(pair.y, pair.x, p.y > 0.0);
    let q = abs(p) - half + radius;
    let s = select(vec2(-1.0), vec2(1.0), p >= vec2(0.0));
    let v = q * s;
    if radius <= 0.0 && any(q > vec2(0.0)) {
        // The corner is nearest where the pixel's foot on both sides falls
        // past it. On screen: `J⁻¹` (up to its determinant) of the local
        // offset and of each side's direction past the corner.
        let adjugate = mat2x2(vec2(dy.y, -dx.y), vec2(-dy.x, dx.x));
        let offset = adjugate * v;
        if dot(offset, adjugate * vec2(0.0, s.y)) >= 0.0 && dot(offset, adjugate * vec2(s.x, 0.0)) >= 0.0 {
            return length(offset) / max(abs(dx.x * dy.y - dx.y * dy.x), 1.0e-12);
        }
    }
    if radius > 0.0 && all(q > vec2(0.0)) {
        let n = normalize(v);
        return (length(q) - radius) / max(length(vec2(dot(n, dx), dot(n, dy))), 1.0e-6);
    }
    let across = max(vec2(length(vec2(dx.x, dy.x)), length(vec2(dx.y, dy.y))), vec2(1.0e-6));
    return max((q.x - radius) / across.x, (q.y - radius) / across.y);
}
