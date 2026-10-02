//! Renderer-facing glyph IR: what `NanaRenderer::text` draws.
//!
//! Nothing here names a shaping backend. A resolver turns a
//! `nana_text::TextLayout` into [`NanaGlyphRun`]s over a flat [`PlacedGlyph`]
//! arena, and every stage below this module only ever sees that IR plus
//! [`GlyphRasterKey`].
//!
//! The split that matters is which facts are in the raster key and which are
//! not. Text color, node opacity and the scene transform change where a glyph
//! lands and how it is tinted, never the bitmap, so they stay on the run and
//! out of the key — otherwise one label in two colors would rasterize twice.

use std::ops::Range;

/// An OpenType glyph index. `u32` rather than `u16`: this is a cross-layer id
/// and widening it later would be an ABI break, the same call
/// [`nana_text::ShapedGlyph`](nana_text::ShapedGlyph) made.
pub(super) type GlyphId = u32;

/// A face as the renderer names it.
///
/// Issued by the glyph resolver's font source (see
/// [`super::raster::SwashGlyphRasterizer::intern`]), dense and `Copy`. It is
/// deliberately **not** a shaping backend's face id: #99 replaces the source
/// that issues it without touching the raster cache or the atlas.
///
/// Only meaningful together with the font generation it was issued under.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct GlyphFontId(pub u32);

/// The axis coordinates a face was instantiated at, as one id.
///
/// Zero is the face's own default instance. Two runs that differ only here are
/// different outlines, so this is part of the raster key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct GlyphVariationId(pub u64);

/// Synthesis, hinting and orientation the rasterizer applies, as opposed to
/// what the face already carries. Part of the raster key: each flag changes
/// the bitmap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) struct GlyphSynthesis(pub u8);

impl GlyphSynthesis {
    pub(super) const NONE: Self = Self(0);
    /// Skew an upright face to stand in for a missing italic.
    pub(super) const FAKE_ITALIC: Self = Self(1 << 0);
    pub(super) const DISABLE_HINTING: Self = Self(1 << 1);
    /// Snap the subpixel offset: a bitmap face must land on whole pixels.
    pub(super) const PIXEL_FONT: Self = Self(1 << 2);
    /// Embolden an upright face to stand in for a weight it cannot reach.
    /// Reported by the font layer when the requested weight is out of the
    /// face's range and no other face covers it.
    pub(super) const FAKE_BOLD: Self = Self(1 << 3);
    /// Turn the outline 90° clockwise: a sideways run of a vertical line
    /// (#59), whose glyphs lie on their side with their tops facing the line's
    /// block end. Not synthesis in the font sense, but the same kind of fact —
    /// a transform the rasterizer applies that changes the bitmap — so it
    /// rides in the same key field.
    pub(super) const ROTATE_CW: Self = Self(1 << 4);

    pub(super) const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }

    #[must_use]
    pub(super) const fn with(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }
}

/// What a run asks the rasterizer to produce.
///
/// The *request*, not the result: a color face answers a [`Mask`](Self::Mask)
/// request with a color bitmap, and [`super::raster::GlyphImage::format`] is
/// what actually came back.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum GlyphRenderMode {
    /// Grayscale coverage, with color faces passed through as color bitmaps.
    Mask,
    /// Per-subpixel (LCD / ClearType) coverage for a panel whose subpixels
    /// run red, green, blue left to right. Color faces still answer with
    /// color bitmaps.
    SubpixelRgb,
    /// The same for a panel whose subpixels run blue, green, red.
    SubpixelBgr,
}

/// Fractional pen placement, quantized to quarter pixels.
///
/// Unquantized placement would make the raster key unbounded: a label sliding
/// under an animation would rasterize a new bitmap every frame. Quarters are
/// the same bucketing the reference path used, so glyph positioning does not
/// shift under this rewrite.
///
/// [`Self::split`] is cosmic-text's `SubpixelBin::new` — the thresholds, the
/// branch order and the carry into the whole pixel are all kept, because what
/// makes the migration invisible is that a glyph lands in the same bin it
/// used to. Derived from cosmic-text, Copyright (c) 2022 System76, MIT OR
/// Apache-2.0 (<https://github.com/pop-os/cosmic-text>), the same licence pair
/// NanaUI is under. See `docs/reference/third-party.md`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) struct SubpixelBin(u8);

impl SubpixelBin {
    /// Split a physical-pixel coordinate into its whole pixel and its bin.
    pub(super) fn split(position: f32) -> (i32, Self) {
        let trunc = position as i32;
        let fract = position - trunc as f32;
        if position.is_sign_negative() {
            if fract > -0.125 {
                (trunc, Self(0))
            } else if fract > -0.375 {
                (trunc - 1, Self(3))
            } else if fract > -0.625 {
                (trunc - 1, Self(2))
            } else if fract > -0.875 {
                (trunc - 1, Self(1))
            } else {
                (trunc - 1, Self(0))
            }
        } else if fract < 0.125 {
            (trunc, Self(0))
        } else if fract < 0.375 {
            (trunc, Self(1))
        } else if fract < 0.625 {
            (trunc, Self(2))
        } else if fract < 0.875 {
            (trunc, Self(3))
        } else {
            (trunc + 1, Self(0))
        }
    }

    /// The pen offset this bin rasterizes at, in pixels.
    pub(super) fn as_float(self) -> f32 {
        f32::from(self.0) * 0.25
    }
}

/// The raster size is the shaped size, to the bit.
///
/// Bucketing it was tried and rejected: at 1/64 px a 15.6px heading shifted to
/// 15.59375 and its antialiased edges moved by up to 11/255, which is a
/// rendering change this phase has no reason to make. What bounds the key
/// space is the cache's byte budget and its LRU, not a coarser key — and the
/// shaped paragraphs above already key on the same exact size, so a scale
/// animation was never going to reuse a bitmap anyway.
pub(super) fn size_bits(size_px: f32) -> u32 {
    size_px.max(0.0).to_bits()
}

pub(super) fn size_from_bits(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Everything that decides a glyph's bitmap, and nothing that does not.
///
/// The font generation is part of the key rather than a reason to flush: a
/// `@font-face` registration reissues face ids, and an entry keyed by the old
/// generation can never be handed to the new one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct GlyphRasterKey {
    pub font: GlyphFontId,
    pub font_generation: u32,
    pub variation: GlyphVariationId,
    pub glyph: GlyphId,
    /// Raster size, see [`size_bits`].
    pub size_bits: u32,
    pub subpixel_x: SubpixelBin,
    pub subpixel_y: SubpixelBin,
    pub synthesis: GlyphSynthesis,
    pub mode: GlyphRenderMode,
}

/// One glyph's pen position in physical pixels, fraction included.
///
/// The fraction is not folded away here: [`GlyphRasterKey`] bins it and the
/// whole-pixel remainder is what the instance is placed at, so the two cannot
/// disagree about where the bitmap was rendered for.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlacedGlyph {
    pub glyph: GlyphId,
    pub x: f32,
    pub y: f32,
}

/// A maximal span of glyphs sharing a face instance, a size and a color.
///
/// `glyphs` indexes the [`NanaGlyphBuffer`] arena rather than owning a `Vec`:
/// a paragraph is many short runs and a per-run allocation is the cost this IR
/// exists to avoid.
#[derive(Clone, Debug)]
pub(super) struct NanaGlyphRun {
    pub font: GlyphFontId,
    pub font_generation: u32,
    pub variation: GlyphVariationId,
    pub size_bits: u32,
    pub synthesis: GlyphSynthesis,
    pub render_mode: GlyphRenderMode,
    /// sRGB, opacity already folded in. Not in the raster key.
    pub color: [f32; 4],
    pub glyphs: Range<u32>,
}

impl NanaGlyphRun {
    /// The raster key for one of this run's glyphs at `placed`.
    ///
    /// Returns the whole-pixel pen position alongside, because the two are
    /// derived from the same split and separating them invites drift.
    pub(super) fn raster_key(&self, placed: &PlacedGlyph) -> (GlyphRasterKey, [i32; 2]) {
        let (x, subpixel_x) = SubpixelBin::split(placed.x);
        let (y, subpixel_y) = SubpixelBin::split(placed.y);
        (
            GlyphRasterKey {
                font: self.font,
                font_generation: self.font_generation,
                variation: self.variation,
                glyph: placed.glyph,
                size_bits: self.size_bits,
                subpixel_x,
                subpixel_y,
                synthesis: self.synthesis,
                mode: self.render_mode,
            },
            [x, y],
        )
    }
}

/// One paragraph's runs and the arena their ranges index.
///
/// Reused across frames by [`clear`](Self::clear) rather than reallocated: a
/// text-heavy frame resolves tens of thousands of glyphs and this is the
/// buffer they land in.
#[derive(Default, Debug)]
pub(super) struct NanaGlyphBuffer {
    pub runs: Vec<NanaGlyphRun>,
    pub glyphs: Vec<PlacedGlyph>,
}

impl NanaGlyphBuffer {
    pub(super) fn clear(&mut self) {
        self.runs.clear();
        self.glyphs.clear();
    }

    pub(super) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub(super) fn glyphs_of(&self, run: &NanaGlyphRun) -> &[PlacedGlyph] {
        let start = run.glyphs.start as usize;
        let end = (run.glyphs.end as usize).min(self.glyphs.len());
        if start >= end {
            return &[];
        }
        &self.glyphs[start..end]
    }

    /// Append `glyph` to the run matching `descriptor`, opening one when the
    /// last run is a different instance, size or color.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push(
        &mut self,
        font: GlyphFontId,
        font_generation: u32,
        variation: GlyphVariationId,
        size_bits: u32,
        synthesis: GlyphSynthesis,
        render_mode: GlyphRenderMode,
        color: [f32; 4],
        glyph: PlacedGlyph,
    ) {
        let index = self.glyphs.len() as u32;
        self.glyphs.push(glyph);
        if let Some(run) = self.runs.last_mut()
            && run.font == font
            && run.font_generation == font_generation
            && run.variation == variation
            && run.size_bits == size_bits
            && run.synthesis == synthesis
            && run.render_mode == render_mode
            && run.color == color
            && run.glyphs.end == index
        {
            run.glyphs.end = index + 1;
            return;
        }
        self.runs.push(NanaGlyphRun {
            font,
            font_generation,
            variation,
            size_bits,
            synthesis,
            render_mode,
            color,
            glyphs: index..index + 1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subpixel_bins_round_to_quarters_and_carry_into_the_whole_pixel() {
        assert_eq!(SubpixelBin::split(4.0), (4, SubpixelBin(0)));
        assert_eq!(SubpixelBin::split(4.3), (4, SubpixelBin(1)));
        assert_eq!(SubpixelBin::split(4.5), (4, SubpixelBin(2)));
        assert_eq!(SubpixelBin::split(4.95), (5, SubpixelBin(0)));
        assert_eq!(SubpixelBin::split(-0.5), (-1, SubpixelBin(2)));
        assert_eq!(SubpixelBin(3).as_float(), 0.75);
    }

    #[test]
    fn a_fractional_dpi_size_survives_the_raster_size_bucket_exactly() {
        for size in [13.0f32, 16.0, 20.0] {
            for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
                let physical = size * scale;
                assert_eq!(
                    size_from_bits(size_bits(physical)),
                    physical,
                    "{size} at {scale} must not drift through the raster bucket"
                );
            }
        }
    }

    #[test]
    fn color_leaves_the_raster_key_alone_but_opens_a_new_run() {
        let mut buffer = NanaGlyphBuffer::default();
        let glyph = PlacedGlyph {
            glyph: 7,
            x: 1.0,
            y: 2.0,
        };
        for color in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]] {
            buffer.push(
                GlyphFontId(1),
                3,
                GlyphVariationId(0),
                size_bits(16.0),
                GlyphSynthesis::NONE,
                GlyphRenderMode::Mask,
                color,
                glyph,
            );
        }
        assert_eq!(buffer.runs.len(), 2, "a color change starts a run");
        let (first, _) = buffer.runs[0].raster_key(&glyph);
        let (second, _) = buffer.runs[1].raster_key(&glyph);
        assert_eq!(
            first, second,
            "the same glyph in two colors must share one bitmap"
        );
    }

    #[test]
    fn glyphs_of_a_run_are_the_ones_pushed_under_it() {
        let mut buffer = NanaGlyphBuffer::default();
        for index in 0..3u32 {
            buffer.push(
                GlyphFontId(1),
                0,
                GlyphVariationId(0),
                size_bits(16.0),
                GlyphSynthesis::NONE,
                GlyphRenderMode::Mask,
                [1.0; 4],
                PlacedGlyph {
                    glyph: index,
                    x: index as f32,
                    y: 0.0,
                },
            );
        }
        assert_eq!(buffer.runs.len(), 1);
        let glyphs = buffer.glyphs_of(&buffer.runs[0]);
        assert_eq!(glyphs.len(), 3);
        assert_eq!(glyphs[2].glyph, 2);
    }
}
