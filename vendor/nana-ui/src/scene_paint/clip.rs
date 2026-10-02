use nana_ui_scene::SceneRect;

use crate::PhysicalRect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl LogicalRect {
    pub const fn from_xywh(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn viewport(origin: [f32; 2], logical_size: [f32; 2]) -> Self {
        Self {
            x: origin[0],
            y: origin[1],
            width: logical_size[0].max(0.0),
            height: logical_size[1].max(0.0),
        }
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let width = right - x;
        let height = bottom - y;
        if width <= 0.0 || height <= 0.0 {
            None
        } else {
            Some(Self {
                x,
                y,
                width,
                height,
            })
        }
    }

    pub fn to_core(self) -> crate::LogicalRect {
        crate::LogicalRect::new(self.x, self.y, self.width, self.height)
    }
}

pub(super) const IDENTITY_AFFINE: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// CSS/Canvas `matrix(a, b, c, d, e, f)`: `x' = ax + cy + e`, `y' = bx + dy + f`.
pub(super) fn transform_point([a, b, c, d, e, f]: [f32; 6], x: f32, y: f32) -> [f32; 2] {
    transform_point_projective([a, b, c, d, e, f], [0.0, 0.0], x, y)
}

pub(super) fn transform_point_projective(
    [a, b, c, d, e, f]: [f32; 6],
    [g, h]: [f32; 2],
    x: f32,
    y: f32,
) -> [f32; 2] {
    let xp = a * x + c * y + e;
    let yp = b * x + d * y + f;
    let w = g * x + h * y + 1.0;
    if !w.is_finite() || w.abs() < 1e-8 {
        return [xp, yp];
    }
    [xp / w, yp / w]
}

/// Where scene space lands in paint space, and the device pixels per logical
/// px of the grid [`paint_transform`] snaps a translation to — a primitive's
/// and each of its clips' alike, so a clip stays on the pixel of what it clips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaintOrigin {
    pub offset: [f32; 2],
    pub scale: f32,
}

impl PaintOrigin {
    pub(super) fn new(offset: [f32; 2], scale: f32) -> Self {
        Self { offset, scale }
    }
}

/// A 1x paint at `offset`, which is what the geometry tests draw.
#[cfg(test)]
impl From<[f32; 2]> for PaintOrigin {
    fn from(offset: [f32; 2]) -> Self {
        Self::new(offset, 1.0)
    }
}

/// Scene origin is a post-translation of every primitive and clip.
/// For a homography, that is `a' = a + ox*g` (not merely `e += ox`).
pub(super) fn paint_affine(transform: [f32; 6], origin: PaintOrigin) -> [f32; 6] {
    paint_transform(transform, [0.0, 0.0], origin).0
}

/// `transform` in paint space, a pure translation snapped to whole device
/// pixels (see [`snap_translation`]).
pub(super) fn paint_transform(
    [a, b, c, d, e, f]: [f32; 6],
    [g, h]: [f32; 2],
    origin: PaintOrigin,
) -> ([f32; 6], [f32; 2]) {
    let [ox, oy] = origin.offset;
    let affine = [
        a + ox * g,
        b + oy * g,
        c + ox * h,
        d + oy * h,
        e + ox,
        f + oy,
    ];
    (snap_translation(affine, [g, h], origin.scale), [g, h])
}

/// A pure translation moved to the nearest whole device pixel; anything else
/// as it is (#223).
///
/// The scroll offset stays fractional — a trackpad's slow deltas are smaller
/// than a pixel — and what is drawn is snapped, as a browser presents a
/// composited scroll. Snapping the final translation rather than the scroll
/// term keeps everything under a scroll on the same whole pixels (a label's
/// glyphs at the same phase, text on its background) and a frozen row, which
/// cancels the scroll with a translation of its own, exactly where it was.
/// Hit testing keeps the exact offset: under half a device pixel apart.
pub(super) fn snap_translation(affine: [f32; 6], persp: [f32; 2], scale: f32) -> [f32; 6] {
    if !is_translation_projective(affine, persp) {
        return affine;
    }
    let [a, b, c, d, e, f] = affine;
    [
        a,
        b,
        c,
        d,
        (e * scale).round() / scale,
        (f * scale).round() / scale,
    ]
}

#[cfg(test)]
pub(super) fn is_translation(transform: [f32; 6]) -> bool {
    is_translation_projective(transform, [0.0, 0.0])
}

pub(crate) fn is_translation_projective([a, b, c, d, _, _]: [f32; 6], [g, h]: [f32; 2]) -> bool {
    a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0 && g.abs() <= 1e-8 && h.abs() <= 1e-8
}

/// Pixel origin of a span of `logical_extent` centered on `logical_center`.
///
/// Icons and vertically centered text share this so a 12px glyph and a 12px
/// line box snap to the same physical top.
pub(super) fn snap_centered_origin(
    logical_center: f32,
    logical_extent: f32,
    scale: f32,
) -> (f32, f32) {
    let px = (logical_extent * scale).round().max(1.0);
    let origin = round_on_grid(logical_center * scale - px * 0.5);
    (origin, px)
}

/// `local + translation` for a snapped translation, added in device pixels.
///
/// Added in logical px, the snapped translation's own rounding error — a few
/// thousandths of a pixel forty thousand pixels down a list — would push a
/// position exactly on a tie of the grid (a 16 px icon at an odd y at 150%, a
/// clip edge on a pixel at 120%) to either side of it from frame to frame.
/// Here what is left is an ulp of the result, which [`grid_slack`] covers.
pub(crate) fn on_grid(local: f32, translation: f32, scale: f32) -> f32 {
    (local * scale + (translation * scale).round()) / scale
}

/// `bounds` moved by the translation of `affine`, on the grid (see [`on_grid`]).
pub(super) fn translated_on_grid(bounds: LogicalRect, affine: [f32; 6], scale: f32) -> LogicalRect {
    LogicalRect {
        x: on_grid(bounds.x, affine[4], scale),
        y: on_grid(bounds.y, affine[5], scale),
        width: bounds.width.max(0.0),
        height: bounds.height.max(0.0),
    }
}

/// How far a position on the grid can come back from its pixel: a few ulps,
/// and never less than the quad shader's own edge slack (`quad_solid.wgsl`).
fn grid_slack(value: f32) -> f32 {
    (value.abs() * f32::EPSILON * 8.0).max(1.0e-3)
}

/// The device pixels a span covers, an edge a few ulps past a pixel boundary
/// counted as on it: the column beyond must not flicker in and out.
pub(crate) fn covered_span(start: f32, end: f32) -> (f32, f32) {
    (
        (start + grid_slack(start)).floor(),
        (end - grid_slack(end)).ceil(),
    )
}

/// `round`, a tie a few ulps short rounding up with it — up, as the quad
/// shader's edges do, not away from zero.
fn round_on_grid(value: f32) -> f32 {
    (value + grid_slack(value)).round()
}

fn near_zero(value: f32) -> bool {
    value.abs() <= 1e-6
}

/// True when a transformed rect stays axis-aligned (scale, translation, 90°).
/// GPU scissor matches the painted parallelogram in that case.
pub(super) fn is_axis_aligned([a, b, c, d, _, _]: [f32; 6]) -> bool {
    (near_zero(b) && near_zero(c)) || (near_zero(a) && near_zero(d))
}

/// Inverse of CSS/Canvas `matrix(a, b, c, d, e, f)`.
pub(super) fn invert_affine([a, b, c, d, e, f]: [f32; 6]) -> Option<[f32; 6]> {
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let ia = d * inv_det;
    let ib = -b * inv_det;
    let ic = -c * inv_det;
    let id = a * inv_det;
    Some([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)])
}

/// Innermost non-axis-aligned clip parallelogram; extras dest-composite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FragmentClip {
    pub rect: [f32; 4],
    pub inv_abcd: [f32; 4],
    pub inv_ef: [f32; 2],
    /// Uniform inset-round radius in logical px; zero keeps axis-aligned rect clip.
    pub corner_radius: f32,
    /// `clip-path: polygon(...)` vertex count (≤ 8) in [`Self::rect`] local space.
    pub polygon_count: u8,
    pub polygon: [[f32; 2]; 8],
}

impl FragmentClip {
    /// Identity inverse + a rect that contains any paint-space coordinate.
    pub const PASS: Self = Self {
        rect: [-1.0e7, -1.0e7, 2.0e7, 2.0e7],
        inv_abcd: [1.0, 0.0, 0.0, 1.0],
        inv_ef: [0.0, 0.0],
        corner_radius: 0.0,
        polygon_count: 0,
        polygon: [[0.0; 2]; 8],
    };

    /// Degenerate clip: no fragment is inside.
    pub const REJECT: Self = Self {
        rect: [0.0, 0.0, -1.0, -1.0],
        inv_abcd: [1.0, 0.0, 0.0, 1.0],
        inv_ef: [0.0, 0.0],
        corner_radius: 0.0,
        polygon_count: 0,
        polygon: [[0.0; 2]; 8],
    };

    /// Exact bit pattern of the clip, for use as a resource-cache key.
    pub(super) fn to_bits(self) -> [u32; 30] {
        let mut bits = [0u32; 30];
        for (slot, value) in bits.iter_mut().zip(
            self.rect
                .iter()
                .chain(self.inv_abcd.iter())
                .chain(self.inv_ef.iter())
                .chain(std::iter::once(&self.corner_radius))
                .chain(std::iter::once(&(self.polygon_count as f32))),
        ) {
            *slot = value.to_bits();
        }
        let mut index = 14;
        for point in self.polygon {
            for coord in point {
                bits[index] = coord.to_bits();
                index += 1;
            }
        }
        bits
    }

    fn from_local(
        bounds: SceneRect,
        inverse: [f32; 6],
        corner_radius: f32,
        polygon: Option<&[[f32; 2]]>,
    ) -> Self {
        let mut clip = Self {
            rect: [bounds.x, bounds.y, bounds.width, bounds.height],
            inv_abcd: [inverse[0], inverse[1], inverse[2], inverse[3]],
            inv_ef: [inverse[4], inverse[5]],
            corner_radius,
            polygon_count: 0,
            polygon: [[0.0; 2]; 8],
        };
        if let Some(points) = polygon {
            if points.len() == 1 {
                clip.polygon_count = 1;
            } else if points.len() >= 3 {
                let count = points.len().min(8);
                clip.polygon_count = count as u8;
                for (slot, point) in clip.polygon.iter_mut().zip(points.iter().take(8)) {
                    *slot = *point;
                }
            }
        }
        clip
    }

    /// Dest-group / affine text / icons sample physical pixels. Scale
    /// rect/radius/polygon; only `inv_ef` scales (linear inverse is invariant).
    pub(super) fn for_physical_pixels(self, scale: f32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let mut polygon = self.polygon;
        if self.polygon_count > 0 {
            for point in polygon.iter_mut().take(self.polygon_count as usize) {
                point[0] *= scale;
                point[1] *= scale;
            }
        }
        Self {
            rect: [
                self.rect[0] * scale,
                self.rect[1] * scale,
                self.rect[2] * scale,
                self.rect[3] * scale,
            ],
            inv_abcd: self.inv_abcd,
            inv_ef: [self.inv_ef[0] * scale, self.inv_ef[1] * scale],
            corner_radius: self.corner_radius * scale,
            polygon_count: self.polygon_count,
            polygon,
        }
    }
}

fn axis_aligned_rounded_clip(
    clip: &nana_ui_scene::ClipRegion,
    origin: PaintOrigin,
) -> Option<FragmentClip> {
    if clip.transform.is_projective() {
        return None;
    }
    let affine = paint_affine(clip.transform.0, origin);
    if !is_axis_aligned(affine) || clip.corner_radius <= 0.0 {
        return None;
    }
    let inverse = invert_affine(affine)?;
    Some(FragmentClip::from_local(
        clip.bounds,
        inverse,
        clip.corner_radius,
        None,
    ))
}

fn axis_aligned_rounded_fragment_clips(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    clips
        .iter()
        .filter_map(|clip| axis_aligned_rounded_clip(clip, origin))
        .collect()
}

fn polygon_clip(clip: &nana_ui_scene::ClipRegion, origin: PaintOrigin) -> Option<FragmentClip> {
    let points = clip.polygon_clip.as_ref()?;
    if points.len() != 1 && points.len() < 3 {
        return None;
    }
    if clip.transform.is_projective() {
        return None;
    }
    let affine = paint_affine(clip.transform.0, origin);
    let inverse = invert_affine(affine)?;
    Some(FragmentClip::from_local(
        clip.bounds,
        inverse,
        clip.corner_radius,
        Some(points),
    ))
}

pub(super) fn polygon_fragment_clips(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    clips
        .iter()
        .filter_map(|clip| polygon_clip(clip, origin))
        .collect()
}

/// Outer-to-inner non-axis-aligned clips. Empty when every clip is axis-aligned
/// (GPU scissor is exact) or the list is empty.
fn rotated_clip(clip: &nana_ui_scene::ClipRegion, origin: PaintOrigin) -> Option<FragmentClip> {
    if clip.transform.is_projective() {
        return None;
    }
    let affine = paint_affine(clip.transform.0, origin);
    if is_axis_aligned(affine) {
        return None;
    }
    Some(match invert_affine(affine) {
        Some(inverse) => FragmentClip::from_local(
            clip.bounds,
            inverse,
            clip.corner_radius,
            clip.polygon_clip.as_deref(),
        ),
        None => FragmentClip::REJECT,
    })
}

pub(super) fn rotated_fragment_clips(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    clips
        .iter()
        .filter_map(|clip| rotated_clip(clip, origin))
        .collect()
}

/// `bounds` (layout space) under `transform`, as a fragment clip.
pub(super) fn local_rect_clip(
    bounds: nana_ui_scene::SceneRect,
    transform: nana_ui_scene::AffineTransform,
    origin: PaintOrigin,
) -> FragmentClip {
    if transform.is_projective() {
        return FragmentClip::PASS;
    }
    match invert_affine(paint_affine(transform.0, origin)) {
        Some(inverse) => FragmentClip::from_local(bounds, inverse, 0.0, None),
        None => FragmentClip::REJECT,
    }
}

/// `clips` without the quad's own polygon, which its shader tests against
/// its fill's edge and a clip would ramp a second time. A styled quad's chain
/// closes with that `clip-path`, its points taken relative to their own
/// bounding box; a quad given a polygon by its component (a donut chart's
/// segment) has none there, and its chain stays whole.
pub(super) fn without_own_polygon<'a>(
    clips: &'a [nana_ui_scene::ClipRegion],
    bounds: SceneRect,
    transform: nana_ui_scene::AffineTransform,
    own: &[[f32; 2]],
) -> &'a [nana_ui_scene::ClipRegion] {
    let Some((last, outer)) = clips.split_last() else {
        return clips;
    };
    let is_own = last.transform == transform
        && last.polygon_clip.as_ref().is_some_and(|points| {
            points.len() == own.len()
                && points.iter().zip(own).all(|(p, q)| {
                    (last.bounds.x + p[0] - bounds.x - q[0]).abs() <= 1.0e-3
                        && (last.bounds.y + p[1] - bounds.y - q[1]).abs() <= 1.0e-3
                })
        });
    if is_own { outer } else { clips }
}

/// Innermost rotated clip for Quad/Mesh/Text/HostTexture vertex attrs.
/// Extra outers are [`extra_fragment_clips`] and dest-composited.
pub(super) fn fragment_clip(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> FragmentClip {
    // Innermost first, so the answer is the first match rather than the last
    // element of three lists. Every primitive of every frame asks this, and a
    // `FragmentClip` is thirty words: collecting the other candidates only to
    // drop them is a heap allocation per clipped node per frame.
    if let Some(rotated) = clips
        .iter()
        .rev()
        .find_map(|clip| rotated_clip(clip, origin))
    {
        return rotated;
    }
    if let Some(polygon) = clips
        .iter()
        .rev()
        .find_map(|clip| polygon_clip(clip, origin))
    {
        return polygon;
    }
    clips
        .iter()
        .rev()
        .find_map(|clip| axis_aligned_rounded_clip(clip, origin))
        .unwrap_or(FragmentClip::PASS)
}

/// Dest extras around Quad/Mesh/Text/HostTexture. Innermost rotated/rounded
/// stays in vertex attrs. Polygons dest-wrap for quads; mesh drops the inner
/// polygon ([`mesh_extra_fragment_clips`]). Unique by bit pattern.
pub(super) fn extra_fragment_clips(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    // The overwhelming majority of nodes are under plain axis-aligned clips,
    // which the scissor handles exactly. Answering those costs one cheap
    // predicate per clip and no `FragmentClip` at all — thirty words each.
    if !clips.iter().any(needs_fragment_test) {
        return Vec::new();
    }
    // Each candidate list once, and the innermost derived from them rather
    // than by asking `fragment_clip` — which would classify every clip three
    // more times for an answer these lists already hold. The overwhelming
    // majority of nodes are under plain axis-aligned clips, which the scissor
    // handles exactly, and all three come back empty without allocating.
    let rotated = rotated_fragment_clips(clips, origin);
    let rounded = axis_aligned_rounded_fragment_clips(clips, origin);
    let polygons = polygon_fragment_clips(clips, origin);
    let Some(inner) = rotated
        .last()
        .or_else(|| polygons.last())
        .or_else(|| rounded.last())
    else {
        return Vec::new();
    };
    let inner_bits = inner.to_bits();
    let mut extras = Vec::new();
    for clip in rotated {
        if clip.to_bits() != inner_bits {
            push_unique_clip(&mut extras, clip);
        }
    }
    for clip in rounded {
        if clip.to_bits() != inner_bits {
            push_unique_clip(&mut extras, clip);
        }
    }
    for clip in polygons {
        push_unique_clip(&mut extras, clip);
    }
    extras
}

/// Every clip the scissor cannot express — rotated, polygonal or rounded —
/// outer to inner, each once: all of them dest-wrap a Custom node, which has
/// no fragment clip of its own.
pub(super) fn all_fragment_clips(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    let mut dest = Vec::new();
    for clip in clips.iter().filter(|clip| needs_fragment_test(clip)) {
        if let Some(fragment) = rotated_clip(clip, origin)
            .or_else(|| polygon_clip(clip, origin))
            .or_else(|| axis_aligned_rounded_clip(clip, origin))
        {
            push_unique_clip(&mut dest, fragment);
        }
    }
    dest
}

/// Whether this clip needs more than the scissor: rounded, polygonal or
/// rotated.
///
/// The scene origin does not enter it. For a non-projective clip the origin is
/// a post-translation, which cannot turn an axis-aligned rectangle into a
/// rotated one — so the six multiplies of `paint_affine` would only ever
/// confirm what `clip.transform` already says.
fn needs_fragment_test(clip: &nana_ui_scene::ClipRegion) -> bool {
    if clip.polygon_clip.is_some() || clip.corner_radius > 0.0 {
        return true;
    }
    if clip.transform.is_projective() {
        return false;
    }
    !is_axis_aligned(clip.transform.0)
}

fn push_unique_clip(extras: &mut Vec<FragmentClip>, clip: FragmentClip) {
    let bits = clip.to_bits();
    if extras.iter().any(|existing| existing.to_bits() == bits) {
        return;
    }
    extras.push(clip);
}

/// Mesh GpuClip and text own the innermost polygon or ellipse; drop it from
/// dest extras, or an ellipse's edge ramp would fade twice.
pub(super) fn mesh_extra_fragment_clips(
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Vec<FragmentClip> {
    let mut extras = extra_fragment_clips(clips, origin);
    let inner = fragment_clip(clips, origin);
    if inner.polygon_count > 0 {
        extras.retain(|clip| clip.to_bits() != inner.to_bits());
    }
    extras
}

#[cfg(test)]
pub(super) fn point_in_fragment_clip(x: f32, y: f32, clip: FragmentClip) -> bool {
    let [local_x, local_y] = transform_point(
        [
            clip.inv_abcd[0],
            clip.inv_abcd[1],
            clip.inv_abcd[2],
            clip.inv_abcd[3],
            clip.inv_ef[0],
            clip.inv_ef[1],
        ],
        x,
        y,
    );
    if local_x < clip.rect[0]
        || local_y < clip.rect[1]
        || local_x > clip.rect[0] + clip.rect[2]
        || local_y > clip.rect[1] + clip.rect[3]
    {
        return false;
    }
    if clip.polygon_count == 1 {
        let rel_x = local_x - clip.rect[0];
        let rel_y = local_y - clip.rect[1];
        let half_w = clip.rect[2] * 0.5;
        let half_h = clip.rect[3] * 0.5;
        let nx = (rel_x - half_w) / half_w.max(1.0e-4);
        let ny = (rel_y - half_h) / half_h.max(1.0e-4);
        return nx * nx + ny * ny <= 1.0;
    }
    if clip.polygon_count >= 3 {
        let rel_x = local_x - clip.rect[0];
        let rel_y = local_y - clip.rect[1];
        if !point_in_polygon(rel_x, rel_y, clip.polygon_count, &clip.polygon) {
            return false;
        }
    }
    if clip.corner_radius <= 0.0 {
        return true;
    }
    let rel_x = local_x - clip.rect[0];
    let rel_y = local_y - clip.rect[1];
    let half_w = clip.rect[2] * 0.5;
    let half_h = clip.rect[3] * 0.5;
    let center_x = rel_x - half_w;
    let center_y = rel_y - half_h;
    let radius = clip.corner_radius.min(half_w).min(half_h);
    let q_x = center_x.abs() - half_w + radius;
    let q_y = center_y.abs() - half_h + radius;
    let outside = q_x.max(0.0).hypot(q_y.max(0.0)) - radius;
    let inside = q_x.max(q_y).min(0.0);
    inside + outside <= 0.0
}

#[cfg(test)]
fn point_in_polygon(x: f32, y: f32, count: u8, polygon: &[[f32; 2]; 8]) -> bool {
    if count < 3 {
        return true;
    }
    let mut winding = 0i32;
    for i in 0..count {
        let j = (i + 1) % count;
        let [x0, y0] = polygon[i as usize];
        let [x1, y1] = polygon[j as usize];
        if y0 <= y {
            if y1 > y {
                let cross = (x1 - x0) * (y - y0) - (x - x0) * (y1 - y0);
                if cross > 0.0 {
                    winding += 1;
                }
            }
        } else if y1 <= y {
            let cross = (x1 - x0) * (y - y0) - (x - x0) * (y1 - y0);
            if cross < 0.0 {
                winding -= 1;
            }
        }
    }
    winding != 0
}

/// Intersection of every rotated parallelogram. Axis-aligned clips are not in
/// `clips` here; they stay on the GPU scissor.
#[cfg(test)]
pub(super) fn point_in_fragment_clips(x: f32, y: f32, clips: &[FragmentClip]) -> bool {
    clips
        .iter()
        .copied()
        .all(|clip| point_in_fragment_clip(x, y, clip))
}

pub(super) fn local_rect(bounds: SceneRect) -> LogicalRect {
    LogicalRect {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    }
}

/// Axis-aligned bounds of a rectangle after the (possibly projective) transform.
pub(super) fn transformed_aabb(bounds: LogicalRect, transform: [f32; 6]) -> LogicalRect {
    transformed_aabb_projective(bounds, transform, [0.0, 0.0])
}

pub(super) fn transformed_aabb_projective(
    bounds: LogicalRect,
    transform: [f32; 6],
    persp: [f32; 2],
) -> LogicalRect {
    // A translation is what nearly every node is under, including the scene
    // origin a scrolled viewport folds in, and its box is the same box moved.
    // Four projective corners and a min/max fold to discover that is the kind
    // of thing a per-primitive path cannot afford.
    if is_translation_projective(transform, persp) {
        return LogicalRect {
            x: bounds.x + transform[4],
            y: bounds.y + transform[5],
            width: bounds.width.max(0.0),
            height: bounds.height.max(0.0),
        };
    }
    let corners = [
        transform_point_projective(transform, persp, bounds.x, bounds.y),
        transform_point_projective(transform, persp, bounds.x + bounds.width, bounds.y),
        transform_point_projective(transform, persp, bounds.x, bounds.y + bounds.height),
        transform_point_projective(
            transform,
            persp,
            bounds.x + bounds.width,
            bounds.y + bounds.height,
        ),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for [x, y] in corners {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    LogicalRect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

#[cfg(test)]
pub(super) fn translated_rect(
    bounds: SceneRect,
    transform: [f32; 6],
    origin: PaintOrigin,
) -> LogicalRect {
    translated_rect_projective(bounds, transform, [0.0, 0.0], origin)
}

pub(super) fn translated_rect_projective(
    bounds: SceneRect,
    transform: [f32; 6],
    persp: [f32; 2],
    origin: PaintOrigin,
) -> LogicalRect {
    let (matrix, persp) = paint_transform(transform, persp, origin);
    if is_translation_projective(matrix, persp) {
        return translated_on_grid(local_rect(bounds), matrix, origin.scale);
    }
    transformed_aabb_projective(local_rect(bounds), matrix, persp)
}

pub(super) fn paint_origin(target_origin: [f32; 2], scene_origin: [f32; 2]) -> [f32; 2] {
    [
        target_origin[0] - scene_origin[0],
        target_origin[1] - scene_origin[1],
    ]
}

/// Transformed AABB scissor for GPU `set_scissor_rect`.
///
/// Rotated/sheared clips still contribute their AABB here as a coarse reject.
/// Quad, Mesh, affine text, and HostTexture overflow clip to [`fragment_clip`]
/// (innermost parallelogram). Extra outer rotated clips dest-composite through
/// [`extra_fragment_clips`]. Custom nodes wrap every such clip in the same
/// dest path ([`all_fragment_clips`]) so renderers need not implement one.
/// Rounded HostTexture clip is the sibling Quad SDF, not this intersection.
pub(super) fn intersect_clips(
    viewport: LogicalRect,
    clips: &[nana_ui_scene::ClipRegion],
    origin: PaintOrigin,
) -> Option<LogicalRect> {
    clips.iter().try_fold(viewport, |visible, clip| {
        visible.intersection(translated_rect_projective(
            clip.bounds,
            clip.transform.0,
            clip.transform.1,
            origin,
        ))
    })
}

pub(super) fn physical_scissor(
    clip: LogicalRect,
    scale_factor: f32,
    target: [u32; 2],
) -> Option<PhysicalRect> {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let (left, right) = covered_span(clip.x * scale, (clip.x + clip.width) * scale);
    let (top, bottom) = covered_span(clip.y * scale, (clip.y + clip.height) * scale);
    let x = left.max(0.0).min(target[0] as f32);
    let y = top.max(0.0).min(target[1] as f32);
    let max_x = right.max(0.0).min(target[0] as f32);
    let max_y = bottom.max(0.0).min(target[1] as f32);
    let width = (max_x - x).max(0.0);
    let height = (max_y - y).max(0.0);
    if width < 1.0 || height < 1.0 {
        None
    } else {
        Some(PhysicalRect {
            x: x as u32,
            y: y as u32,
            width: width as u32,
            height: height as u32,
        })
    }
}

pub(super) fn physical_bounds(
    bounds: LogicalRect,
    scale_factor: f32,
    clip: PhysicalRect,
) -> PhysicalRect {
    let Some(slot) = physical_scissor(bounds, scale_factor, [u32::MAX, u32::MAX]) else {
        return PhysicalRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    };
    intersect_physical(slot, clip)
}

/// An empty rect covers no pixel, so it never overlaps anything.
pub(super) fn overlaps_physical(left: PhysicalRect, right: PhysicalRect) -> bool {
    if left.width == 0 || left.height == 0 || right.width == 0 || right.height == 0 {
        return false;
    }
    left.x < right.x.saturating_add(right.width)
        && right.x < left.x.saturating_add(left.width)
        && left.y < right.y.saturating_add(right.height)
        && right.y < left.y.saturating_add(left.height)
}

pub(super) fn union_physical(left: PhysicalRect, right: PhysicalRect) -> PhysicalRect {
    if left.width == 0 || left.height == 0 {
        return right;
    }
    if right.width == 0 || right.height == 0 {
        return left;
    }
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    let right_edge = left
        .x
        .saturating_add(left.width)
        .max(right.x.saturating_add(right.width));
    let bottom_edge = left
        .y
        .saturating_add(left.height)
        .max(right.y.saturating_add(right.height));
    PhysicalRect {
        x,
        y,
        width: right_edge - x,
        height: bottom_edge - y,
    }
}

pub(super) fn intersect_physical(left: PhysicalRect, right: PhysicalRect) -> PhysicalRect {
    let x = left.x.max(right.x);
    let y = left.y.max(right.y);
    let right_edge = left
        .x
        .saturating_add(left.width)
        .min(right.x.saturating_add(right.width));
    let bottom_edge = left
        .y
        .saturating_add(left.height)
        .min(right.y.saturating_add(right.height));
    PhysicalRect {
        x,
        y,
        width: right_edge.saturating_sub(x),
        height: bottom_edge.saturating_sub(y),
    }
}

#[cfg(test)]
mod tests {
    use nana_ui_scene::{AffineTransform, ClipRegion, SceneRect};

    use super::*;

    #[test]
    fn snap_centered_origin_rounds_the_span_then_the_top() {
        let (origin, px) = snap_centered_origin(14.0, 12.0, 2.0);
        assert_eq!(px, 24.0);
        assert_eq!(origin, 16.0);
    }

    #[test]
    fn paint_origin_is_target_minus_scene() {
        assert_eq!(paint_origin([0.0, 0.0], [40.0, 20.0]), [-40.0, -20.0]);
        assert_eq!(paint_origin([200.0, 10.0], [40.0, 20.0]), [160.0, -10.0]);
        assert_eq!(paint_origin([200.0, 10.0], [0.0, 0.0]), [200.0, 10.0]);
    }

    #[test]
    fn scene_origin_subtracts_layout_origin() {
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [40.0, 20.0]));
        let rect = translated_rect(
            SceneRect {
                x: 52.0,
                y: 48.0,
                width: 80.0,
                height: 32.0,
            },
            AffineTransform::IDENTITY.0,
            origin,
        );
        assert_eq!(rect.x, 12.0);
        assert_eq!(rect.y, 28.0);
        assert_eq!(rect.width, 80.0);
        assert_eq!(rect.height, 32.0);
    }

    #[test]
    fn clip_translation_intersects_viewport() {
        let viewport = LogicalRect::viewport([0.0, 0.0], [100.0, 80.0]);
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 50.0,
            },
            transform: AffineTransform::from_matrix([1.0, 0.0, 0.0, 1.0, 5.0, 8.0]),
            corner_radius: 0.0,
            polygon_clip: None,
        }];
        let visible = intersect_clips(
            viewport,
            &clips,
            PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0])),
        )
        .unwrap();
        assert_eq!(visible.x, 15.0);
        assert_eq!(visible.y, 18.0);
        assert_eq!(visible.width, 50.0);
        assert_eq!(visible.height, 50.0);
    }

    #[test]
    fn translated_clip_outside_viewport_is_empty() {
        let viewport = LogicalRect::viewport([0.0, 0.0], [100.0, 80.0]);
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            transform: AffineTransform::from_matrix([1.0, 0.0, 0.0, 1.0, 200.0, 0.0]),
            corner_radius: 0.0,
            polygon_clip: None,
        }];
        assert!(
            intersect_clips(
                viewport,
                &clips,
                PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]))
            )
            .is_none()
        );
    }

    #[test]
    fn ancestor_clips_intersect_and_honor_scene_origin() {
        let viewport = LogicalRect::viewport([0.0, 0.0], [160.0, 80.0]);
        let clips = [
            ClipRegion {
                bounds: SceneRect {
                    x: 40.0,
                    y: 20.0,
                    width: 80.0,
                    height: 40.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 0.0,
                polygon_clip: None,
            },
            ClipRegion {
                bounds: SceneRect {
                    x: 50.0,
                    y: 30.0,
                    width: 20.0,
                    height: 10.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 0.0,
                polygon_clip: None,
            },
        ];
        let visible = intersect_clips(
            viewport,
            &clips,
            PaintOrigin::from(paint_origin([0.0, 0.0], [40.0, 20.0])),
        )
        .unwrap();
        assert_eq!(visible.x, 10.0);
        assert_eq!(visible.y, 10.0);
        assert_eq!(visible.width, 20.0);
        assert_eq!(visible.height, 10.0);
    }

    #[test]
    fn rotated_rect_aabb_covers_transformed_corners() {
        let bounds = LogicalRect::from_xywh(10.0, 20.0, 8.0, 4.0);
        let identity = transformed_aabb(bounds, IDENTITY_AFFINE);
        assert_eq!(identity, bounds);

        // 90° around origin: (x, y) -> (-y, x)
        let rotated = transformed_aabb(bounds, [0.0, 1.0, -1.0, 0.0, 0.0, 0.0]);
        assert_eq!(rotated.x, -24.0);
        assert_eq!(rotated.y, 10.0);
        assert_eq!(rotated.width, 4.0);
        assert_eq!(rotated.height, 8.0);
    }

    #[test]
    fn paint_affine_adds_scene_origin_to_translation() {
        assert!(is_translation(IDENTITY_AFFINE));
        assert!(!is_translation([0.0, 1.0, -1.0, 0.0, 5.0, 7.0]));
        assert_eq!(
            paint_affine(
                [1.0, 0.0, 0.0, 1.0, 4.0, 6.0],
                PaintOrigin::from([-40.0, -20.0])
            ),
            [1.0, 0.0, 0.0, 1.0, -36.0, -14.0]
        );
        assert_eq!(
            transform_point([0.0, 1.0, -1.0, 0.0, 5.0, 7.0], 3.0, 4.0),
            [1.0, 10.0]
        );
    }

    #[test]
    fn a_translation_lands_on_whole_device_pixels_and_nothing_else_moves() {
        for scale in [1.0f32, 1.1, 1.2, 1.25, 1.5, 1.75, 2.0] {
            for offset in [0.37f32, -0.37, 0.5, -12.63, 4095.81, -8191.2] {
                let origin = PaintOrigin::new([0.0, 0.0], scale);
                let snapped = paint_affine([1.0, 0.0, 0.0, 1.0, offset, -offset], origin);
                for (axis, logical) in [(4, offset), (5, -offset)] {
                    let physical = snapped[axis] * scale;
                    assert!(
                        (physical - physical.round()).abs() <= 1e-3,
                        "{offset} at {scale}x must land on a device pixel, got {physical}"
                    );
                    assert!(
                        (physical - logical * scale).abs() <= 0.5 + 1e-3,
                        "and on the nearest one: {physical} for {}",
                        logical * scale
                    );
                }
            }
        }
        // The scene origin is part of the translation that is snapped: what
        // lands on the grid is the paint-space position.
        let origin = PaintOrigin::new([-0.25, 0.0], 1.0);
        assert_eq!(paint_affine([1.0, 0.0, 0.0, 1.0, 3.0, 0.0], origin)[4], 3.0);
        // Anything but a translation is left to its own snapping rules.
        let origin = PaintOrigin::new([0.0, 0.0], 1.0);
        let turned = [0.0, 1.0, -1.0, 0.0, 5.37, 7.37];
        assert_eq!(paint_affine(turned, origin), turned);
        let zoomed = [2.0, 0.0, 0.0, 2.0, 5.37, 7.37];
        assert_eq!(paint_affine(zoomed, origin), zoomed);
        let tilted = paint_transform([1.0, 0.0, 0.0, 1.0, 5.37, 7.37], [0.01, 0.0], origin);
        assert_eq!(tilted.0[4], 5.37);
    }

    #[test]
    fn a_snapped_translation_moves_ties_and_clip_edges_by_whole_pixels() {
        // Positions a whole or a half device pixel from the grid are where the
        // rounding error a snapped translation carries would pick a side: a
        // 16 px icon at an odd y at 150%, a clip edge on a pixel at 120%.
        // Scrolled — a little, or forty thousand pixels down a list, where
        // the translation's own ulp is thousandths of a pixel — each must
        // move by exactly the translation's pixels, the way a quad does.
        for scale in [1.1f32, 1.2, 1.25, 1.5, 1.75] {
            let origin = PaintOrigin::new([0.0, 0.0], scale);
            for base in (100..300).chain(40_000..40_200) {
                let y = base as f32;
                let scrolled = |offset: f32| {
                    paint_affine([1.0, 0.0, 0.0, 1.0, 0.0, 100.0 - y - offset], origin)
                };
                let still = scrolled(0.0);
                let icon = |f: f32| snap_centered_origin(on_grid(y + 8.0, f, scale), 16.0, scale).0;
                let edges = |f: f32| {
                    let rect = translated_rect_projective(
                        SceneRect {
                            x: y,
                            y,
                            width: 30.0,
                            height: 30.0,
                        },
                        [1.0, 0.0, 0.0, 1.0, 0.0, f],
                        [0.0; 2],
                        PaintOrigin::new([0.0, 0.0], scale),
                    );
                    let rect = LogicalRect { x: 0.0, ..rect };
                    let scissor = physical_scissor(rect, scale, [u32::MAX; 2]).unwrap();
                    (scissor.y as f32, scissor.height)
                };
                let (top, height) = edges(still[5]);
                for step in 1..100 {
                    let f = scrolled(step as f32 * 0.37)[5];
                    let pixels = ((f - still[5]) * scale).round();
                    assert_eq!(
                        icon(f),
                        icon(still[5]) + pixels,
                        "an icon at {y} scrolled {f} at {scale}x"
                    );
                    assert_eq!(
                        edges(f),
                        (top + pixels, height),
                        "a clip at {y} scrolled {f} at {scale}x"
                    );
                }
            }
        }
        // A tie above the target's top edge rounds the way the quad shader's
        // edges do — up — not away from zero.
        assert_eq!(
            snap_centered_origin(-1.5 / 1.5 + 8.0 / 1.5, 16.0 / 1.5, 1.5).0,
            -1.0
        );
    }

    #[test]
    fn paint_transform_bakes_scene_origin_into_homography() {
        let matrix = [1.0, 0.0, 0.0, 1.0, 4.0, 6.0];
        let persp = [0.01, 0.0];
        let origin = [-40.0, -20.0];
        let (baked, out_persp) = paint_transform(matrix, persp, PaintOrigin::from(origin));
        assert_eq!(out_persp, persp);
        assert!((baked[0] - (1.0 + origin[0] * persp[0])).abs() < 1e-6);
        assert_eq!(baked[4], 4.0 + origin[0]);
        assert_eq!(baked[5], 6.0 + origin[1]);
        let p = transform_point_projective(matrix, persp, 10.0, 20.0);
        let q = transform_point_projective(baked, out_persp, 10.0, 20.0);
        assert!((q[0] - (p[0] + origin[0])).abs() < 1e-4);
        assert!((q[1] - (p[1] + origin[1])).abs() < 1e-4);
    }

    #[test]
    fn physical_scissor_is_in_target_pixels() {
        let clip = LogicalRect::from_xywh(10.25, 20.5, 100.5, 50.25);
        let scissor = physical_scissor(clip, 1.5, [400, 300]).unwrap();
        assert_eq!(scissor.x, 15);
        assert_eq!(scissor.y, 30);
        assert_eq!(scissor.width, 152);
        assert_eq!(scissor.height, 77);
    }

    #[test]
    fn invert_affine_roundtrips_and_rejects_singular() {
        let original = [2.0, 0.5, -0.25, 3.0, 4.0, -6.0];
        let inverse = invert_affine(original).unwrap();
        let [x, y] = transform_point(original, 7.0, 11.0);
        let [rx, ry] = transform_point(inverse, x, y);
        assert!((rx - 7.0).abs() < 1e-5 && (ry - 11.0).abs() < 1e-5);
        assert_eq!(invert_affine(IDENTITY_AFFINE), Some(IDENTITY_AFFINE));
        assert!(invert_affine([1.0, 0.0, 2.0, 0.0, 3.0, 4.0]).is_none());
    }

    #[test]
    fn axis_aligned_includes_scale_and_right_angles() {
        assert!(is_axis_aligned(IDENTITY_AFFINE));
        assert!(is_axis_aligned([2.0, 0.0, 0.0, 0.5, 3.0, 4.0]));
        assert!(is_axis_aligned([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]));
        assert!(!is_axis_aligned([0.707, 0.707, -0.707, 0.707, 0.0, 0.0]));
        assert!(!is_axis_aligned([1.0, 0.0, 0.5, 1.0, 0.0, 0.0]));
    }

    #[test]
    fn fragment_clip_rejects_aabb_corner_outside_rotated_rect() {
        let viewport = LogicalRect::viewport([0.0, 0.0], [64.0, 64.0]);
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 16.0,
                y: 16.0,
                width: 32.0,
                height: 32.0,
            },
            transform: AffineTransform::from_matrix(
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    e: 0.0,
                    f: 0.0,
                }
                .around_center(16.0, 16.0, 32.0, 32.0),
            ),
            corner_radius: 0.0,
            polygon_clip: None,
        }];
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        let aabb = intersect_clips(viewport, &clips, origin).unwrap();
        let clip = fragment_clip(&clips, origin);
        assert!(point_in_fragment_clip(32.0, 32.0, clip));

        let corner_x = aabb.x + 1.0;
        let corner_y = aabb.y + 1.0;
        assert!(
            corner_x >= aabb.x
                && corner_y >= aabb.y
                && corner_x <= aabb.x + aabb.width
                && corner_y <= aabb.y + aabb.height,
            "probe must sit in the transformed AABB scissor"
        );
        assert!(
            !point_in_fragment_clip(corner_x, corner_y, clip),
            "AABB corner {corner_x},{corner_y} must stay outside the rotated rect"
        );
    }

    #[test]
    fn fragment_clip_passes_when_every_clip_is_axis_aligned() {
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            transform: AffineTransform::IDENTITY,
            corner_radius: 0.0,
            polygon_clip: None,
        }];
        assert_eq!(
            fragment_clip(
                &clips,
                PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]))
            ),
            FragmentClip::PASS
        );
    }

    #[test]
    fn fragment_clip_physical_pixels_match_logical_probe() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 16.0,
                y: 16.0,
                width: 32.0,
                height: 32.0,
            },
            transform: AffineTransform::from_matrix(
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    e: 0.0,
                    f: 0.0,
                }
                .around_center(16.0, 16.0, 32.0, 32.0),
            ),
            corner_radius: 0.0,
            polygon_clip: None,
        }];
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        let clip = fragment_clip(&clips, origin);
        let physical = clip.for_physical_pixels(2.0);
        assert!(point_in_fragment_clip(32.0, 32.0, clip));
        assert!(point_in_fragment_clip(64.0, 64.0, physical));
        assert!(!point_in_fragment_clip(1.0, 1.0, clip));
        assert!(!point_in_fragment_clip(2.0, 2.0, physical));
    }

    #[test]
    fn axis_aligned_rect_clip_rejects_outside() {
        let clip = FragmentClip::from_local(
            SceneRect {
                x: 16.0,
                y: 16.0,
                width: 32.0,
                height: 32.0,
            },
            IDENTITY_AFFINE,
            0.0,
            None,
        );
        assert!(point_in_fragment_clip(32.0, 32.0, clip));
        assert!(
            !point_in_fragment_clip(8.0, 8.0, clip),
            "single AABB reject must drop points outside the rect"
        );
        assert!(!point_in_fragment_clip(56.0, 32.0, clip));
    }

    #[test]
    fn only_a_quad_s_own_clip_path_leaves_its_chain() {
        let rect = |x, y, width, height| SceneRect {
            x,
            y,
            width,
            height,
        };
        let quad = rect(10.0, 20.0, 100.0, 80.0);
        let own = [[20.0, 10.0], [90.0, 40.0], [30.0, 70.0]];
        let parent = ClipRegion {
            corner_radius: 8.0,
            ..ClipRegion::axis_aligned(rect(0.0, 0.0, 60.0, 60.0), AffineTransform::IDENTITY)
        };
        // As `clip_path_region` resolves it: relative to its bounding box.
        let clip_path = ClipRegion {
            polygon_clip: Some(vec![[0.0, 0.0], [70.0, 30.0], [10.0, 60.0]]),
            ..ClipRegion::axis_aligned(rect(30.0, 30.0, 70.0, 60.0), AffineTransform::IDENTITY)
        };
        let styled = [parent.clone(), clip_path.clone()];
        assert_eq!(
            without_own_polygon(&styled, quad, AffineTransform::IDENTITY, &own),
            &styled[..1]
        );
        // A donut chart's segment: its polygon is its own, not its chain's.
        let segment = [clip_path.clone(), parent];
        assert_eq!(
            without_own_polygon(&segment, quad, AffineTransform::IDENTITY, &own),
            &segment[..]
        );
        assert!(without_own_polygon(&[], quad, AffineTransform::IDENTITY, &own).is_empty());
        let moved = AffineTransform([1.0, 0.0, 0.0, 1.0, 5.0, 0.0], [0.0, 0.0]);
        assert_eq!(
            without_own_polygon(&styled, quad, moved, &own),
            &styled[..],
            "another space's polygon"
        );
    }

    #[test]
    fn fragment_clip_uses_axis_aligned_polygon() {
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            transform: AffineTransform::IDENTITY,
            corner_radius: 0.0,
            polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
        }];
        let clip = fragment_clip(
            &clips,
            PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0])),
        );
        assert!(
            clip.polygon_count >= 3,
            "ancestor polygon must reach FragmentClip, not PASS AABB"
        );
        assert!(point_in_fragment_clip(32.0, 24.0, clip));
        assert!(!point_in_fragment_clip(4.0, 60.0, clip));
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        assert_eq!(
            extra_fragment_clips(&clips, origin).len(),
            1,
            "quads dest-wrap the ancestor polygon (vertex locations full)"
        );
        assert!(
            mesh_extra_fragment_clips(&clips, origin).is_empty(),
            "mesh GpuClip owns the innermost polygon; no dest pass"
        );
    }

    #[test]
    fn overflow_aabb_plus_polygon_stays_scissor_and_gpu_clip() {
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        let clips = [
            ClipRegion::axis_aligned(
                SceneRect {
                    x: 20.0,
                    y: 20.0,
                    width: 24.0,
                    height: 24.0,
                },
                AffineTransform::IDENTITY,
            ),
            ClipRegion {
                bounds: SceneRect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 0.0,
                polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
            },
        ];
        let inner = fragment_clip(&clips, origin);
        assert!(
            inner.polygon_count >= 3,
            "innermost polygon must stay in GpuClip"
        );
        assert_eq!(
            extra_fragment_clips(&clips, origin).len(),
            1,
            "quads dest-wrap only the polygon; overflow AABB is scissor"
        );
        assert!(
            mesh_extra_fragment_clips(&clips, origin).is_empty(),
            "mesh dest must stay empty: overflow is scissor, polygon is GpuClip"
        );
    }

    #[test]
    fn inset_round_plus_polygon_keeps_round_as_dest() {
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        let clips = [
            ClipRegion {
                bounds: SceneRect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 16.0,
                polygon_clip: None,
            },
            ClipRegion {
                bounds: SceneRect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 0.0,
                polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
            },
        ];
        let inner = fragment_clip(&clips, origin);
        assert!(
            inner.polygon_count >= 3,
            "polygon wins fragment_clip over axis-aligned inset(round)"
        );
        let extras = extra_fragment_clips(&clips, origin);
        assert_eq!(
            extras.len(),
            2,
            "quads dest-wrap displaced inset(round) and the polygon, got {extras:?}"
        );
        assert!(
            extras
                .iter()
                .any(|clip| clip.corner_radius > 0.0 && clip.polygon_count < 3),
            "displaced inset(round) must dest-composite, got {extras:?}"
        );
        let mesh = mesh_extra_fragment_clips(&clips, origin);
        assert_eq!(mesh.len(), 1, "mesh dest is only the displaced round");
        assert!(
            mesh[0].corner_radius > 0.0 && mesh[0].polygon_count < 3,
            "mesh GpuClip owns the polygon; dest keeps ancestor inset(round), got {mesh:?}"
        );
    }

    #[test]
    fn an_ellipse_is_tested_once_whichever_side_owns_it() {
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        let clips = [ClipRegion::ellipse(
            SceneRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 32.0,
            },
            AffineTransform::IDENTITY,
        )];
        let inner = fragment_clip(&clips, origin);
        assert_eq!(inner.polygon_count, 1);
        assert_eq!(extra_fragment_clips(&clips, origin), vec![inner]);
        assert!(mesh_extra_fragment_clips(&clips, origin).is_empty());
    }

    #[test]
    fn rotated_outer_polygon_is_not_dest_wrapped_twice() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0]));
        let rotated = AffineTransform::from_matrix(
            nana_ui_core::PaintTransform {
                a: k,
                b: k,
                c: -k,
                d: k,
                ..Default::default()
            }
            .around_center(0.0, 0.0, 64.0, 64.0),
        );
        let clips = [
            ClipRegion {
                bounds: SceneRect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                transform: rotated,
                corner_radius: 0.0,
                polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
            },
            ClipRegion {
                bounds: SceneRect {
                    x: 16.0,
                    y: 16.0,
                    width: 32.0,
                    height: 32.0,
                },
                transform: rotated,
                corner_radius: 0.0,
                polygon_clip: None,
            },
        ];
        let extras = extra_fragment_clips(&clips, origin);
        let polygon_dests = extras.iter().filter(|clip| clip.polygon_count >= 3).count();
        assert_eq!(
            polygon_dests, 1,
            "rotated outer polygon must dest-wrap once, got {extras:?}"
        );
        let mesh = mesh_extra_fragment_clips(&clips, origin);
        assert_eq!(mesh.len(), 1);
        assert!(
            mesh[0].polygon_count >= 3,
            "innermost is rotated overflow; mesh dest is the outer polygon"
        );
    }

    #[test]
    fn polygon_fragment_clip_accepts_triangle_interior() {
        let clip = polygon_fragment_clips(
            &[ClipRegion {
                bounds: SceneRect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 0.0,
                polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
            }],
            PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0])),
        )
        .pop()
        .expect("triangle clip");
        assert!(point_in_fragment_clip(32.0, 24.0, clip));
        assert!(!point_in_fragment_clip(4.0, 60.0, clip));
    }

    #[test]
    fn rotated_inset_round_rejects_local_corner_inside_sharp() {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        let clips = [ClipRegion {
            bounds: SceneRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            transform: AffineTransform::from_matrix(
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    ..Default::default()
                }
                .around_center(0.0, 0.0, 64.0, 64.0),
            ),
            corner_radius: 24.0,
            polygon_clip: None,
        }];
        let origin = PaintOrigin::from(paint_origin([0.0, 0.0], [-16.0, -16.0]));
        let rounded = fragment_clip(&clips, origin);
        let mut sharp = rounded;
        sharp.corner_radius = 0.0;
        // Dest (48.5, 8.5) matches the GPU pixel center.
        let px = 48.5;
        let py = 8.5;
        assert!(
            point_in_fragment_clip(px, py, sharp),
            "probe must sit inside the sharp rotated rect; clip={sharp:?}"
        );
        assert!(
            !point_in_fragment_clip(px, py, rounded),
            "r=24 SDF must reject the local corner; radius=0 would keep it"
        );
    }

    #[test]
    fn polygon_fragment_clip_physical_pixels_match_logical() {
        let clip = polygon_fragment_clips(
            &[ClipRegion {
                bounds: SceneRect {
                    x: 0.0,
                    y: 0.0,
                    width: 64.0,
                    height: 64.0,
                },
                transform: AffineTransform::IDENTITY,
                corner_radius: 0.0,
                polygon_clip: Some(vec![[0.0, 0.0], [64.0, 0.0], [32.0, 64.0]]),
            }],
            PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0])),
        )
        .pop()
        .expect("triangle clip");
        let physical = clip.for_physical_pixels(1.0);
        assert!(point_in_fragment_clip(32.0, 24.0, clip));
        assert!(point_in_fragment_clip(32.0, 24.0, physical));
        assert!(!point_in_fragment_clip(4.0, 60.0, physical));
        let physical_2x = clip.for_physical_pixels(2.0);
        assert!(
            point_in_fragment_clip(64.0, 48.0, physical_2x),
            "2× dest pixels must keep the triangle interior"
        );
        assert!(!point_in_fragment_clip(8.0, 120.0, physical_2x));
    }

    fn nested_rotated_45_clips() -> ([ClipRegion; 2], PaintOrigin) {
        let k = std::f32::consts::FRAC_1_SQRT_2;
        // Outer is the smaller parallelogram so a probe can sit inside the
        // overflowing inner diamond and still miss the outer one.
        let outer = ClipRegion {
            bounds: SceneRect {
                x: 16.0,
                y: 16.0,
                width: 32.0,
                height: 32.0,
            },
            transform: AffineTransform::from_matrix(
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    e: 0.0,
                    f: 0.0,
                }
                .around_center(16.0, 16.0, 32.0, 32.0),
            ),
            corner_radius: 0.0,
            polygon_clip: None,
        };
        let inner = ClipRegion {
            bounds: SceneRect {
                x: 0.0,
                y: 0.0,
                width: 64.0,
                height: 64.0,
            },
            transform: AffineTransform::from_matrix(
                nana_ui_core::PaintTransform {
                    a: k,
                    b: k,
                    c: -k,
                    d: k,
                    e: 0.0,
                    f: 0.0,
                }
                .around_center(0.0, 0.0, 64.0, 64.0),
            ),
            corner_radius: 0.0,
            polygon_clip: None,
        };
        (
            [outer, inner],
            PaintOrigin::from(paint_origin([0.0, 0.0], [0.0, 0.0])),
        )
    }

    #[test]
    fn fragment_clip_uses_innermost_rotated_when_nested() {
        let ([outer, inner], origin) = nested_rotated_45_clips();
        let nested = [outer.clone(), inner.clone()];
        assert_eq!(
            fragment_clip(&nested, origin),
            fragment_clip(std::slice::from_ref(&inner), origin),
            "Quad vertex attrs still carry only the innermost parallelogram"
        );
        assert_eq!(
            extra_fragment_clips(&nested, origin),
            vec![fragment_clip(std::slice::from_ref(&outer), origin)]
        );
        assert!(extra_fragment_clips(std::slice::from_ref(&inner), origin).is_empty());
        assert!(extra_fragment_clips(&[outer], origin).is_empty());

        let rotated = rotated_fragment_clips(&nested, origin);
        assert_eq!(rotated.len(), 2);
        let inner_clip = fragment_clip(&[inner], origin);
        let mut probe = None;
        for y in 0..64u32 {
            for x in 0..64u32 {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                if point_in_fragment_clip(px, py, inner_clip)
                    && !point_in_fragment_clips(px, py, &rotated)
                {
                    probe = Some((px, py));
                    break;
                }
            }
            if probe.is_some() {
                break;
            }
        }
        let (px, py) = probe.expect(
            "nested extra must reject a point inside the inner parallelogram but outside the outer",
        );
        assert!(
            point_in_fragment_clip(px, py, inner_clip),
            "innermost-only would keep ({px},{py})"
        );
        assert!(
            !point_in_fragment_clips(px, py, &rotated),
            "AND of both 45° clips must reject ({px},{py})"
        );
        assert!(point_in_fragment_clips(32.0, 32.0, &rotated));
    }
}
