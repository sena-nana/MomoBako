//! The renderer's own rasterizer boundary.
//!
//! [`GlyphRasterizer`] is the abstraction `NanaRenderer::text` owns; what sits
//! under it is an implementation detail it may replace. The implementation
//! scales outlines with `swash` off the faces `nana-text`'s font layer issued,
//! so there is exactly one font instance resolution in the process: the
//! coordinates a run was *shaped* at are the ones it is *scaled* at.
//!
//! The boundary is deliberately wider than today's needs: a request names a
//! [`GlyphRenderMode`] and an answer names its own [`GlyphImageFormat`], so an
//! LCD, SDF or vector backend is a new arm rather than a new pipeline.

use std::collections::HashMap;
use std::sync::Arc;

use nana_text::font::{AxisCoord, FontInstanceKey};

use super::glyph::{
    GlyphFontId, GlyphRasterKey, GlyphRenderMode, GlyphSynthesis, GlyphVariationId, size_from_bits,
};

/// Synthetic-oblique slant, in degrees: 14°, the angle the previous text
/// backend used, so a face without an italic kept leaning the same way through
/// the #99 cutover.
const OBLIQUE_DEGREES: f32 = 14.0;

/// Synthetic-bold stroke as a fraction of the raster size. Stroke weight is
/// proportional to size in every real face, so faking it with a constant
/// number of pixels would over-embolden captions and under-embolden headings.
const EMBOLDEN_RATIO: f32 = 0.02;

/// How a rasterized glyph's bytes are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum GlyphImageFormat {
    /// One byte of coverage per pixel.
    Mask,
    /// Four bytes per pixel, straight (non-premultiplied) sRGB.
    ColorRgba,
    /// Four bytes per pixel: the coverage of the pixel's red, green and blue
    /// subpixels in screen order, then an unused byte.
    ///
    /// Stored sRGB-*encoded*. These share the color pages, whose format
    /// decodes on sampling, so encoding them here is what makes the shader
    /// read back the coverage the rasterizer produced.
    SubpixelRgb,
}

impl GlyphImageFormat {
    pub(super) const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Mask => 1,
            Self::ColorRgba | Self::SubpixelRgb => 4,
        }
    }
}

/// A rasterized glyph, in the rasterizer's own terms.
///
/// `left` / `top` are the bitmap's offset from the pen position, the sign
/// convention every outline rasterizer uses: the quad sits at
/// `(pen.x + left, pen.y - top)`.
#[derive(Debug)]
pub(super) struct GlyphImage {
    pub format: GlyphImageFormat,
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub data: Vec<u8>,
}

impl GlyphImage {
    pub(super) fn byte_len(&self) -> usize {
        self.data.len()
    }

    /// Whether this glyph covers no pixels — a space, or an outline that
    /// scaled away. Cached like any other answer so it is asked for once.
    pub(super) fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

pub(super) struct GlyphRasterRequest {
    pub key: GlyphRasterKey,
}

/// What `NanaRenderer::text` requires of a glyph raster backend.
pub(super) trait GlyphRasterizer {
    /// Rasterize one glyph, or `None` when the backend cannot produce it at
    /// all (an unknown face, a mode it does not implement). A glyph that is
    /// simply blank answers `Some` with an empty image, so the cache can tell
    /// "nothing to draw" from "ask again".
    fn rasterize(&mut self, request: &GlyphRasterRequest) -> Option<GlyphImage>;
}

/// Distinct axis-coordinate sets kept before the least recently used go. An
/// animated font axis (Issue #85) is a new set on every sampled frame.
pub(super) const VARIATION_CAP: usize = 4096;
/// Frames a set stays after its last use, whatever the count: the frame that
/// interned it and the one after may still rasterize with it.
const VARIATION_PIN_FRAMES: u64 = 2;

struct InternedVariation {
    coords: Arc<[AxisCoord]>,
    last_used: u64,
}

/// `swash` behind the renderer's rasterizer boundary.
///
/// Holds the face table that issues [`GlyphFontId`]s, so the ids the glyph IR
/// carries and the faces this rasterizer can scale cannot drift apart.
pub(super) struct SwashGlyphRasterizer {
    /// The engine whose font set issues these faces. The same one the layouts
    /// being resolved were measured against, so a face id in a run always
    /// names a face this can read bytes for.
    engine: nana_text::SharedTextEngine,
    /// Swash's own scaler, which carries its outline cache. Every *cached
    /// answer* lives in [`super::raster_cache::GlyphRasterCache`], which is
    /// the renderer's; this is only the machinery that produces them.
    context: swash::scale::ScaleContext,
    faces: Vec<nana_text::FontId>,
    index: HashMap<nana_text::FontId, GlyphFontId>,
    /// Axis coordinates behind each [`GlyphVariationId`] a run interned (zero
    /// is the face's default instance). The key carries the id because a
    /// bitmap differs by coordinates; the scaler needs the coordinates
    /// themselves.
    ///
    /// Issued in order and looked up by the coordinates themselves, like the
    /// face ids: a hash of the coordinates would let two sets that collide
    /// draw with the first one's axes and share its bitmaps. An id is never
    /// issued twice, so one evicted here ([`Self::begin_frame`]) cannot come
    /// back naming other coordinates; bitmaps keyed by it just go cold.
    variations: HashMap<GlyphVariationId, InternedVariation>,
    variation_index: HashMap<Arc<[AxisCoord]>, GlyphVariationId>,
    issued_variations: u64,
    /// Frames begun, for the variations' last use.
    frame: u64,
    /// The face asked for last. Runs are contiguous by face, so a paragraph
    /// asks for the same one for every glyph and this keeps the hash lookup
    /// off the per-glyph path.
    recent: Option<(nana_text::FontId, GlyphFontId)>,
}

impl SwashGlyphRasterizer {
    pub(super) fn new(engine: nana_text::SharedTextEngine) -> Self {
        Self {
            engine,
            context: swash::scale::ScaleContext::new(),
            faces: Vec::new(),
            index: HashMap::new(),
            variations: HashMap::new(),
            variation_index: HashMap::new(),
            issued_variations: 0,
            frame: 0,
            recent: None,
        }
    }

    /// The renderer's ids for one face instance.
    ///
    /// Interning the whole instance rather than the face alone is what keeps
    /// the coordinates reachable at raster time: the key carries a
    /// [`GlyphVariationId`], and this is the table that turns it back into the
    /// axis values the scaler needs.
    ///
    /// Face ids are generational and never alias, so an id stays valid across
    /// font-set generations; the generation rides in the raster key instead,
    /// where it invalidates bitmaps without invalidating identity.
    pub(super) fn intern_instance(
        &mut self,
        instance: &FontInstanceKey,
    ) -> (GlyphFontId, GlyphVariationId, GlyphSynthesis) {
        let font = self.intern_face(instance.font);
        let variation = self.intern_variation(&instance.coords);
        let mut synthesis = GlyphSynthesis::NONE;
        if instance.synthesis.bold {
            synthesis = synthesis.with(GlyphSynthesis::FAKE_BOLD);
        }
        if instance.synthesis.oblique {
            synthesis = synthesis.with(GlyphSynthesis::FAKE_ITALIC);
        }
        (font, variation, synthesis)
    }

    fn intern_variation(&mut self, coords: &Arc<[AxisCoord]>) -> GlyphVariationId {
        if coords.is_empty() {
            return GlyphVariationId(0);
        }
        let frame = self.frame;
        if let Some(variation) = self.variation_index.get(coords) {
            if let Some(interned) = self.variations.get_mut(variation) {
                interned.last_used = frame;
            }
            return *variation;
        }
        self.issued_variations += 1;
        let variation = GlyphVariationId(self.issued_variations);
        self.variations.insert(
            variation,
            InternedVariation {
                coords: Arc::clone(coords),
                last_used: frame,
            },
        );
        self.variation_index.insert(Arc::clone(coords), variation);
        variation
    }

    /// Coordinate sets holding an id.
    #[cfg(test)]
    pub(super) fn variation_count(&self) -> usize {
        self.variations.len()
    }

    /// Starts a frame. Past [`VARIATION_CAP`] the least recently used sets
    /// not used in the last [`VARIATION_PIN_FRAMES`] frames go, and their ids
    /// are returned. Nothing rasterizes with one again: a run that shows those
    /// coordinates later interns them afresh, and an entry already built
    /// holds its atlas cells, not the id's coordinates.
    pub(super) fn begin_frame(&mut self) -> Vec<GlyphVariationId> {
        self.frame += 1;
        if self.variations.len() <= VARIATION_CAP {
            return Vec::new();
        }
        let pinned_since = self.frame.saturating_sub(VARIATION_PIN_FRAMES);
        let mut cold: Vec<(u64, GlyphVariationId)> = self
            .variations
            .iter()
            .filter(|(_, interned)| interned.last_used < pinned_since)
            .map(|(id, interned)| (interned.last_used, *id))
            .collect();
        // Down to three quarters, so the scan runs once per many frames
        // rather than on every new set.
        let excess = self.variations.len() - VARIATION_CAP * 3 / 4;
        cold.sort_unstable_by_key(|(last_used, id)| (*last_used, id.0));
        cold.truncate(excess);
        cold.into_iter()
            .map(|(_, id)| {
                if let Some(interned) = self.variations.remove(&id) {
                    self.variation_index.remove(&interned.coords);
                }
                id
            })
            .collect()
    }

    fn intern_face(&mut self, face: nana_text::FontId) -> GlyphFontId {
        if let Some((recent, font)) = self.recent
            && recent == face
        {
            return font;
        }
        let font = match self.index.get(&face) {
            Some(font) => *font,
            None => {
                let font = GlyphFontId(self.faces.len() as u32);
                self.faces.push(face);
                self.index.insert(face, font);
                font
            }
        };
        self.recent = Some((face, font));
        font
    }

    /// The bytes behind a face id this rasterizer issued, for a backend that
    /// scales the same face by other means.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn face_data(
        &self,
        font: GlyphFontId,
    ) -> Option<(nana_text::FontId, nana_text::font::FontData)> {
        let face = *self.faces.get(font.0 as usize)?;
        let engine = crate::text_engine::lock_engine(&self.engine);
        Some((face, engine.fonts().face_data(face)?))
    }

    /// The axis coordinates behind a variation id, empty for the default
    /// instance.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn coords(&self, variation: GlyphVariationId) -> &[AxisCoord] {
        self.variation_coords(variation)
            .map_or(&[], |coords| coords)
    }

    fn variation_coords(&self, variation: GlyphVariationId) -> Option<&Arc<[AxisCoord]>> {
        self.variations
            .get(&variation)
            .map(|interned| &interned.coords)
    }

    #[cfg(test)]
    pub(super) fn face_count(&self) -> usize {
        self.faces.len()
    }
}

impl GlyphRasterizer for SwashGlyphRasterizer {
    fn rasterize(&mut self, request: &GlyphRasterRequest) -> Option<GlyphImage> {
        let key = &request.key;
        let face = *self.faces.get(key.font.0 as usize)?;
        let glyph_id = u16::try_from(key.glyph).ok()?;
        // The blob is cloned out of the engine rather than scaled under its
        // lock: a glyph that misses must not hold the lock every other
        // window's layout needs. `FontData` shares the bytes, so this is a
        // refcount, not a copy of the face.
        let data = {
            let engine = crate::text_engine::lock_engine(&self.engine);
            engine.fonts().face_data(face)?
        };
        let font = swash::FontRef::from_index(data.bytes(), data.index() as usize)?;
        let size = size_from_bits(key.size_bits);
        let coords = self.variation_coords(key.variation).cloned();
        let mut builder = self
            .context
            .builder(font)
            .size(size)
            .hint(!key.synthesis.contains(GlyphSynthesis::DISABLE_HINTING));
        if let Some(coords) = coords {
            builder = builder.variations(
                coords
                    .iter()
                    .map(|coord| (swash::Tag::from_be_bytes(coord.tag), coord.value)),
            );
        }
        let mut scaler = builder.build();
        // A face with no outlines has only strikes, and a strike cannot be
        // shifted by a fraction of a pixel — asking for one blurs it. Asked of
        // the scaler rather than carried as a synthesis flag, because it is a
        // property of the face, not of what the caller wanted.
        let snap_to_pixel =
            !scaler.has_outlines() || key.synthesis.contains(GlyphSynthesis::PIXEL_FONT);
        let mut render = swash::scale::Render::new(&[
            // A color outline with the first palette, then a color strike,
            // then the plain outline. Order matters: an emoji face can have
            // all three, and the first is the one with the palette applied.
            swash::scale::Source::ColorOutline(0),
            swash::scale::Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
            swash::scale::Source::Outline,
        ]);
        let format = match key.mode {
            GlyphRenderMode::Mask => swash::zeno::Format::Alpha,
            GlyphRenderMode::SubpixelRgb | GlyphRenderMode::SubpixelBgr => {
                swash::zeno::Format::Subpixel
            }
        };
        render
            .format(format)
            .offset(subpixel_offset(key, snap_to_pixel));
        let mut transform = None;
        if key.synthesis.contains(GlyphSynthesis::FAKE_ITALIC) {
            transform = Some(swash::zeno::Transform::skew(
                swash::zeno::Angle::from_degrees(OBLIQUE_DEGREES),
                swash::zeno::Angle::from_degrees(0.0),
            ));
        }
        if key.synthesis.contains(GlyphSynthesis::ROTATE_CW) {
            // Font space is y-up, so a quarter turn clockwise on the page
            // sends the outline's x to page-down (font −y) and its y to
            // page-right (font +x): (x, y) → (y, −x). Written out rather than
            // as `rotation(-90°)`, whose `cos` is not exactly zero and would
            // lean every sideways glyph by a hair. After the oblique skew, so
            // a fake italic slants along its own baseline.
            let quarter_turn = swash::zeno::Transform::new(0.0, -1.0, 1.0, 0.0, 0.0, 0.0);
            transform = Some(match transform {
                Some(skew) => skew.then(&quarter_turn),
                None => quarter_turn,
            });
        }
        render.transform(transform);
        if key.synthesis.contains(GlyphSynthesis::FAKE_BOLD) {
            render.embolden(size * EMBOLDEN_RATIO);
        }
        let image = render.render(&mut scaler, glyph_id)?;
        let width = image.placement.width;
        let height = image.placement.height;
        let pixels = (width as usize).saturating_mul(height as usize);
        let format = match image.content {
            swash::scale::image::Content::Mask => GlyphImageFormat::Mask,
            swash::scale::image::Content::Color => GlyphImageFormat::ColorRgba,
            swash::scale::image::Content::SubpixelMask => GlyphImageFormat::SubpixelRgb,
        };
        let bytes = pixels * format.bytes_per_pixel();
        if image.data.len() < bytes {
            return None;
        }
        let mut data = image.data[..bytes].to_vec();
        if format == GlyphImageFormat::SubpixelRgb {
            for pixel in data.as_chunks_mut::<4>().0 {
                let rgb = [pixel[0], pixel[1], pixel[2]];
                encode_subpixel(pixel, rgb, key.mode);
            }
        }
        Some(GlyphImage {
            format,
            width,
            height,
            left: image.placement.left,
            top: image.placement.top,
            data,
        })
    }
}

/// Write one pixel of [`GlyphImageFormat::SubpixelRgb`] from its subpixel
/// coverages in red, green, blue order: reordered for a BGR panel and
/// sRGB-encoded.
pub(super) fn encode_subpixel(pixel: &mut [u8; 4], rgb: [u8; 3], mode: GlyphRenderMode) {
    let [r, g, b] = match mode {
        GlyphRenderMode::SubpixelBgr => [rgb[2], rgb[1], rgb[0]],
        _ => rgb,
    };
    *pixel = [r, g, b, 0].map(|value| SRGB_ENCODE[usize::from(value)]);
}

/// Linear 8-bit coverage to the sRGB-encoded byte that decodes back to it.
static SRGB_ENCODE: std::sync::LazyLock<[u8; 256]> = std::sync::LazyLock::new(|| {
    std::array::from_fn(|value| {
        let c = value as f32 / 255.0;
        let encoded = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round() as u8
    })
});

/// The fractional pen offset a glyph is rendered at, snapped whole for a face
/// that has nothing to shift.
fn subpixel_offset(key: &GlyphRasterKey, snap: bool) -> swash::zeno::Vector {
    let x = key.subpixel_x.as_float();
    let y = key.subpixel_y.as_float();
    if snap {
        swash::zeno::Vector::new(x.round(), y.round())
    } else {
        swash::zeno::Vector::new(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_text::font::Synthesis;

    fn instance(font: nana_text::FontId, coords: Vec<AxisCoord>) -> FontInstanceKey {
        FontInstanceKey {
            font,
            coords: coords.into(),
            synthesis: Synthesis::default(),
        }
    }

    fn faces(count: usize) -> Vec<nana_text::FontId> {
        let engine = crate::text_engine::nana_text_engine();
        let engine = crate::text_engine::lock_engine(&engine);
        engine.fonts().faces().into_iter().take(count).collect()
    }

    #[test]
    fn interning_the_same_face_twice_issues_one_id() {
        let mut rasterizer = SwashGlyphRasterizer::new(crate::text_engine::nana_text_engine());
        let ids = faces(2);
        if ids.is_empty() {
            return;
        }
        let first = rasterizer.intern_instance(&instance(ids[0], Vec::new())).0;
        assert_eq!(
            rasterizer.intern_instance(&instance(ids[0], Vec::new())).0,
            first
        );
        assert_eq!(rasterizer.face_count(), 1);
        if let Some(second) = ids.get(1) {
            assert_ne!(
                rasterizer.intern_instance(&instance(*second, Vec::new())).0,
                first,
                "a different face is a different id"
            );
        }
    }

    #[test]
    fn the_default_instance_and_a_varied_one_are_different_raster_keys() {
        let mut rasterizer = SwashGlyphRasterizer::new(crate::text_engine::nana_text_engine());
        let ids = faces(1);
        if ids.is_empty() {
            return;
        }
        let (_, default, _) = rasterizer.intern_instance(&instance(ids[0], Vec::new()));
        let (_, varied, _) = rasterizer.intern_instance(&instance(
            ids[0],
            vec![AxisCoord {
                tag: *b"wght",
                value: 700.0,
            }],
        ));
        assert_eq!(default, GlyphVariationId(0));
        assert_ne!(varied, default, "coordinates change the bitmap");
        let wide = vec![AxisCoord {
            tag: *b"wdth",
            value: 75.0,
        }];
        let (_, narrowed, _) = rasterizer.intern_instance(&instance(ids[0], wide.clone()));
        assert_ne!(narrowed, varied, "other coordinates are another instance");
        assert_eq!(rasterizer.coords(narrowed), wide.as_slice());
        let (_, again, _) = rasterizer.intern_instance(&instance(ids[0], wide));
        assert_eq!(
            again, narrowed,
            "the same coordinates are the same instance"
        );
        assert_eq!(
            rasterizer.face_count(),
            1,
            "coordinates are not a second face"
        );
    }

    #[test]
    fn synthesis_travels_from_the_font_layer_to_the_raster_key() {
        let mut rasterizer = SwashGlyphRasterizer::new(crate::text_engine::nana_text_engine());
        let ids = faces(1);
        if ids.is_empty() {
            return;
        }
        let mut key = instance(ids[0], Vec::new());
        key.synthesis = Synthesis {
            bold: true,
            oblique: true,
        };
        let (_, _, synthesis) = rasterizer.intern_instance(&key);
        assert!(synthesis.contains(GlyphSynthesis::FAKE_BOLD));
        assert!(synthesis.contains(GlyphSynthesis::FAKE_ITALIC));
    }
}
