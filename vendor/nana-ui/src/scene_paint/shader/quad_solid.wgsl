// Adapted from historical Iced (MIT).
struct SolidVertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
    @location(0) color: vec4<f32>,
    @location(1) pos: vec2<f32>,
    @location(2) scale: vec2<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) border_radius: vec4<f32>,
    @location(5) border_widths: vec4<f32>,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    // Blur and spread radii share one attribute to leave room for the pivot
    // within the 16 vertex attribute limit.
    @location(8) shadow_radii: vec2<f32>,
    @location(9) motion_origin: vec2<f32>,
    @location(10) snap: u32,
    @location(11) affine_abcd: vec4<f32>,
    @location(12) affine_ef: vec4<f32>,
    @location(13) clip_rect: vec4<f32>,
    @location(14) clip_inv_abcd: vec4<f32>,
    @location(15) clip_inv_ef: vec3<f32>,
}

struct SolidVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) border_color: vec4<f32>,
    @location(2) pos: vec2<f32>,
    @location(3) scale: vec2<f32>,
    @location(4) border_radius: vec4<f32>,
    @location(5) border_widths: vec4<f32>,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_blur_radius: f32,
    @location(9) shadow_spread_radius: f32,
    @location(10) local_pos: vec2<f32>,
    // Screen-linear: the fragment's own paint position.
    @location(11) @interpolate(linear) world_pos: vec2<f32>,
    @location(12) clip_rect: vec4<f32>,
    @location(13) clip_inv_abcd: vec4<f32>,
    @location(14) clip_inv_ef: vec3<f32>,
    @location(15) @interpolate(flat) instance_index: u32,
}

fn apply_affine(abcd: vec4<f32>, ef: vec4<f32>, p: vec2<f32>) -> vec2<f32> {
    let x = abcd.x * p.x + abcd.z * p.y + ef.x;
    let y = abcd.y * p.x + abcd.w * p.y + ef.y;
    let w = ef.z * p.x + ef.w * p.y + 1.0;
    if (abs(w) < 1.0e-8) {
        return vec2<f32>(x, y);
    }
    return vec2<f32>(x, y) / w;
}

// Local px the quad reaches past each edge under the transform at `p`: an
// edge moves `det / |other column| / w` device px per local px (Jacobian
// columns × w). One and a half device pixels: the ramp's outer half, plus
// the reach of a pixel's MSAA samples, which a geometry edge crossing the
// pixel would otherwise leave uncovered and so halve that pixel's ramp.
// At least the untransformed half pixel; capped for near-singular transforms.
fn edge_grow(abcd: vec4<f32>, ef: vec4<f32>, p: vec2<f32>) -> vec2<f32> {
    let world = apply_affine(abcd, ef, p);
    let w = abs(ef.z * p.x + ef.w * p.y + 1.0);
    let along_x = vec2(abcd.x - world.x * ef.z, abcd.y - world.y * ef.z);
    let along_y = vec2(abcd.z - world.x * ef.w, abcd.w - world.y * ef.w);
    let det = abs(along_x.x * along_y.y - along_x.y * along_y.x);
    let grow = 1.5 * w * vec2(length(along_y), length(along_x)) / max(det, 1.0e-6);
    return clamp(grow, vec2(0.5), vec2(256.0));
}

// `to_screen` takes a gradient in (along, across) to device pixels, so a
// dash end or dot rim ramps over one device pixel under any scale.
fn border_dash_mask(along: f32, across: f32, width: f32, style: u32, to_screen: mat2x2<f32>) -> f32 {
    if (style == 0u) {
        return 1.0;
    }
    let w = max(width, 1.0);
    if (style == 2u) {
        let period = w * 2.0;
        let nearest = round(along / period) * period;
        let offset = vec2(along - nearest, across);
        let d = length(offset);
        let px = max(length(to_screen * (offset / max(d, 1.0e-6))), 1.0e-6);
        return clamp(0.5 - (d - w * 0.5) / px, 0.0, 1.0);
    }
    let dash = w * 3.0;
    let period = dash + w * 2.0;
    let t = fract(along / period) * period;
    let sd = select(min(t - dash, period - t), -min(t, dash - t), t < dash);
    return clamp(0.5 - sd / max(length(to_screen[0]), 1.0e-6), 0.0, 1.0);
}

@vertex
fn solid_vs_main(input: SolidVertexInput) -> SolidVertexOutput {
    var out: SolidVertexOutput;

    // Outline/inset expansion is packed into instance shadow radii on the CPU so
    // this stage never reads storage (VERTEX_STORAGE is not guaranteed). An
    // inset shadow paints only inside its box, so its quad stays on the box.
    let shadow_blur_radius = input.shadow_radii.x;
    let shadow_spread_radius = input.shadow_radii.y;
    let inset = ((input.snap >> 16u) & PAINT_SHADOW_INSET) != 0u;
    let shadow_offset = select(input.shadow_offset, vec2(0.0), inset);
    let shadow_outset = select(shadow_blur_radius + max(shadow_spread_radius, 0.0), 0.0, inset);
    let pos = input.pos * globals.scale;
    let scale = input.scale * globals.scale;

    // Snap the box, not the quad grown around it: a fractional blur, spread or
    // offset would otherwise slide the box half a pixel.
    var pos_snap = vec2<f32>(0.0, 0.0);
    var scale_snap = vec2<f32>(0.0, 0.0);

    if bool(input.snap & 1u) {
        pos_snap = round(pos + vec2(0.001, 0.001)) - pos;
        scale_snap = round(pos + scale + vec2(0.001, 0.001)) - pos - pos_snap - scale;
    }
    let quad_pos = pos + pos_snap + (min(shadow_offset, vec2(0.0)) - shadow_outset) * globals.scale;
    let quad_size = scale + scale_snap + (abs(shadow_offset) + shadow_outset * 2.0) * globals.scale;

    let border_radius = min(input.border_radius, vec4(min(input.scale.x, input.scale.y) / 2.0));
    let unit = vertex_position(input.vertex_index);
    let transform_id = (input.snap >> 1u) & 0x7fffu;
    let composed = motion_compose_projective(input.affine_abcd, input.affine_ef, input.motion_origin, motion_evaluate(transform_id));
    let corner = quad_pos + unit * quad_size;
    let grow = edge_grow(composed.abcd, composed.ef, corner / globals.scale);
    let local = corner + (unit * 2.0 - 1.0) * grow;
    let logical = local / globals.scale;
    let world = apply_affine(composed.abcd, composed.ef, logical);

    // The homography's `w` as the clip-space w: the rasterizer then
    // interpolates the local position perspective-correctly, which a
    // divided corner and w = 1 would interpolate as an affine map, bending
    // the rounded shape along the quad's diagonal.
    let w = composed.ef.z * logical.x + composed.ef.w * logical.y + 1.0;
    let clip_w = select(1.0, w, w > 1.0e-6);
    out.position = globals.transform * vec4<f32>(world * globals.scale * clip_w, 0.0, clip_w);
    out.color = premultiply(input.color);
    out.border_color = premultiply(input.border_color);
    out.pos = pos + pos_snap;
    out.scale = scale + scale_snap;
    out.border_radius = border_radius * globals.scale;
    out.border_widths = input.border_widths * globals.scale;
    out.shadow_color = premultiply(input.shadow_color);
    out.shadow_offset = input.shadow_offset * globals.scale;
    out.shadow_blur_radius = shadow_blur_radius * globals.scale;
    out.shadow_spread_radius = shadow_spread_radius * globals.scale;
    out.local_pos = local;
    out.world_pos = world;
    out.clip_rect = input.clip_rect;
    out.clip_inv_abcd = input.clip_inv_abcd;
    out.clip_inv_ef = input.clip_inv_ef;
    out.instance_index = input.instance_index;

    return out;
}

@fragment
fn solid_fs_main(
    input: SolidVertexOutput
) -> @location(0) vec4<f32> {
    // Local px per device pixel, for every edge ramp below.
    let local_dx = dpdx(input.local_pos);
    let local_dy = dpdy(input.local_pos);
    let clip_cover = fragment_clip_coverage(
        input.world_pos,
        input.clip_rect,
        input.clip_inv_abcd,
        input.clip_inv_ef.xy,
        input.clip_inv_ef.z,
        0u,
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        vec4<f32>(0.0),
        globals.scale,
    );
    if clip_cover <= 0.0 {
        discard;
    }

    let paint = paint_buffer.items[input.instance_index];
    let local_uv = (input.local_pos - input.pos) / max(input.scale, vec2(0.0001));


    // The box's own `clip-path: polygon()` (which is not dest-wrapped for
    // it), ramped over one device pixel: `local_uv` moves `local_d* / scale`
    // per device pixel.
    var polygon_cover = 1.0;
    if ((paint.flags & PAINT_POLYGON) != 0u) {
        let scale = max(input.scale, vec2(0.0001));
        let edge = polygon_edge_distance(
            local_uv,
            paint.polygon_count,
            paint.poly0,
            paint.poly1,
            paint.poly2,
            paint.poly3,
            local_dx / scale,
            local_dy / scale,
        );
        polygon_cover = clamp(0.5 - select(edge, -edge, point_in_polygon(local_uv, paint)), 0.0, 1.0);
        if polygon_cover <= 0.0 {
            discard;
        }
    }

    var mixed_color: vec4<f32> = compose_quad_fill(input.color, local_uv, paint);

    let outer_p = -(input.local_pos - input.pos - input.scale * 0.5);
    let half = input.scale * 0.5;
    let edge = rounded_box_distance(outer_p, half, input.border_radius, local_dx, local_dy);

    if (max(max(input.border_widths.x, input.border_widths.y), max(input.border_widths.z, input.border_widths.w)) > 0.0) {
        let inner_shift = vec2(
            (input.border_widths.w - input.border_widths.y) * 0.5,
            (input.border_widths.x - input.border_widths.z) * 0.5,
        );
        let inner_size = max(
            input.scale - vec2(
                input.border_widths.w + input.border_widths.y,
                input.border_widths.x + input.border_widths.z,
            ),
            vec2(0.0),
        );
        var inner_radii = max(
            input.border_radius - vec4(
                min(input.border_widths.x, input.border_widths.w),
                min(input.border_widths.x, input.border_widths.y),
                min(input.border_widths.z, input.border_widths.y),
                min(input.border_widths.z, input.border_widths.w),
            ),
            vec4(0.0),
        );
        inner_radii = min(inner_radii, vec4(min(inner_size.x, inner_size.y) * 0.5));
        let inner_p = outer_p + inner_shift;
        let inner_dist = rounded_box_distance(inner_p, inner_size * 0.5, inner_radii, local_dx, local_dy);
        let lp = input.local_pos - input.pos;
        let dt = select(1e8, lp.y / max(input.border_widths.x, 1e-4), input.border_widths.x > 0.0);
        let dr = select(1e8, (input.scale.x - lp.x) / max(input.border_widths.y, 1e-4), input.border_widths.y > 0.0);
        let db = select(1e8, (input.scale.y - lp.y) / max(input.border_widths.z, 1e-4), input.border_widths.z > 0.0);
        let dl = select(1e8, lp.x / max(input.border_widths.w, 1e-4), input.border_widths.w > 0.0);
        var edge_color = input.border_color;
        let nearest = min(min(dt, dr), min(db, dl));
        if (nearest == dr) {
            edge_color = premultiply(paint.border_color_right);
        } else if (nearest == db) {
            edge_color = premultiply(paint.border_color_bottom);
        } else if (nearest == dl) {
            edge_color = premultiply(paint.border_color_left);
        }
        var cover = clamp(0.5 + inner_dist, 0.0, 1.0);
        if (paint.border_styles != 0u) {
            var side: u32 = 0u;
            var along = lp.x;
            var across = lp.y - input.border_widths.x * 0.5;
            var bw = input.border_widths.x;
            // Local directions of `along` and `across` on this side.
            var along_axis = vec2(1.0, 0.0);
            var across_axis = vec2(0.0, 1.0);
            if (nearest == dr) {
                side = 1u;
                along = lp.y;
                across = (input.scale.x - lp.x) - input.border_widths.y * 0.5;
                bw = input.border_widths.y;
                along_axis = vec2(0.0, 1.0);
                across_axis = vec2(-1.0, 0.0);
            } else if (nearest == db) {
                side = 2u;
                along = lp.x;
                across = (input.scale.y - lp.y) - input.border_widths.z * 0.5;
                bw = input.border_widths.z;
                across_axis = vec2(0.0, -1.0);
            } else if (nearest == dl) {
                side = 3u;
                along = lp.y;
                across = lp.x - input.border_widths.w * 0.5;
                bw = input.border_widths.w;
                along_axis = vec2(0.0, 1.0);
                across_axis = vec2(1.0, 0.0);
            }
            let to_screen = mat2x2(
                vec2(dot(along_axis, local_dx), dot(along_axis, local_dy)),
                vec2(dot(across_axis, local_dx), dot(across_axis, local_dy)),
            );
            cover *= border_dash_mask(along, across, bw, (paint.border_styles >> (side * 2u)) & 3u, to_screen);
        }
        mixed_color = mix(mixed_color, edge_color, cover);
    }

    let fill_alpha = clamp(0.5 - edge, 0.0, 1.0);
    var quad_alpha: f32 = fill_alpha;

    // Storage keeps CSS px (same as instance spread before VS scale); the
    // interpolated `shadow_spread_radius` is already physical.
    let outline_px = paint.outline_width * globals.scale;
    if (outline_px > 0.0) {
        // The box grown by the outline, its corners with it.
        let outline_edge = rounded_box_distance(
            outer_p,
            half + outline_px,
            input.border_radius + outline_px,
            local_dx,
            local_dy,
        );
        let cover = clamp(0.5 - outline_edge, 0.0, 1.0);
        let outline_premult = premultiply(paint.outline_color);
        mixed_color = mix(outline_premult, mixed_color, quad_alpha);
        quad_alpha = cover;
    }

    // Where the polygon runs along a side of the box, the nearer of the two
    // edges rules, not both ramps at once.
    let quad_color = mixed_color * min(quad_alpha, polygon_cover);

    // The sample is the curve's, overshoot included; what the quad can show
    // is an opacity, as the CPU compositor clamps it too.
    let motion_opacity = clamp(motion_sample_scalar(motion_evaluate(paint._pad_tail1), 1.0), 0.0, 1.0);
    // The clip's edge coverage scales the premultiplied result the same way.
    let fade = motion_opacity * clip_cover;

    if input.shadow_color.a > 0.0 {
        // Negated for an inset shadow, whose spread shrinks the shape.
        let css_spread = input.shadow_spread_radius - outline_px;
        let shadow_size = max(input.scale + vec2(css_spread * 2.0), vec2(0.0));
        let shadow_radius = max(input.border_radius + vec4(css_spread), vec4(0.0));
        let shadow_p = outer_p + input.shadow_offset;
        let inset = (paint.flags & PAINT_SHADOW_INSET) != 0u;
        // Distance past the shadow's edge, away from where it paints: local
        // px for the blur, device px for an unblurred edge.
        let flip = select(1.0, -1.0, inset);
        let shadow_dist = rounded_box_sdf(shadow_p * 2.0, shadow_size, shadow_radius * 2.0) / 2.0 * flip;
        let shadow_edge = rounded_box_distance(
            shadow_p,
            shadow_size * 0.5,
            shadow_radius,
            local_dx,
            local_dy,
        ) * flip;
        // A blur ramps from full strength `blur` inside the edge to none `blur`
        // past it, as the Path shadow's band, and scales with the transform; an
        // unblurred edge ramps over one device pixel, as the box's own edge does.
        let blur = input.shadow_blur_radius;
        let shadow_alpha = select(
            1.0 - smoothstep(-blur, blur, shadow_dist),
            clamp(0.5 - shadow_edge, 0.0, 1.0),
            blur <= 0.0,
        );
        let under = select(1.0 - quad_alpha, fill_alpha, inset);
        // `clip-path` clips the shadow too.
        return mix(quad_color, input.shadow_color, under * shadow_alpha * polygon_cover) * fade;
    } else {
        return quad_color * fade;
    }
}
