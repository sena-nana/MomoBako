//! DirectWrite behind the rasterizer boundary, on Windows.
//!
//! Windows' own rasterizer, so an outline glyph here has the stems, the
//! vertical grid-fitting and the ClearType filtering every native app on the
//! machine has — and, when the painter asks for subpixel text, ClearType's
//! three coverages per pixel.
//!
//! The faces are still the ones `nana-text`'s font layer issued: their bytes
//! go to DirectWrite through an in-memory loader, so a run is scaled from the
//! very face it was shaped with. Anything DirectWrite is not asked to draw —
//! color glyphs, bitmap-only faces, sideways glyphs of a vertical line — and
//! anything it fails on goes to [`SwashGlyphRasterizer`], which also owns the
//! face table both backends share.

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use nana_text::font::{AxisCoord, FontInstanceKey};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_AXIS_TAG, DWRITE_FONT_AXIS_VALUE,
    DWRITE_FONT_FACE_TYPE_BITMAP, DWRITE_FONT_FACE_TYPE_UNKNOWN, DWRITE_FONT_FILE_TYPE,
    DWRITE_FONT_SIMULATIONS_BOLD, DWRITE_FONT_SIMULATIONS_NONE, DWRITE_FONT_SIMULATIONS_OBLIQUE,
    DWRITE_GLYPH_OFFSET, DWRITE_GLYPH_RUN, DWRITE_GRID_FIT_MODE, DWRITE_GRID_FIT_MODE_DEFAULT,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_OUTLINE_THRESHOLD_ANTIALIASED, DWRITE_PIXEL_GEOMETRY_BGR,
    DWRITE_PIXEL_GEOMETRY_RGB, DWRITE_RENDERING_MODE1, DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC,
    DWRITE_RENDERING_MODE1_OUTLINE, DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE,
    DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE, DWRITE_TEXTURE_ALIASED_1x1, DWRITE_TEXTURE_CLEARTYPE_3x1,
    DWriteCreateFactory, IDWriteFactory, IDWriteFactory5, IDWriteFactory6, IDWriteFontFace,
    IDWriteFontFace2, IDWriteFontFace3, IDWriteFontFile, IDWriteInMemoryFontFileLoader,
    IDWriteRenderingParams, IDWriteRenderingParams1,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FE_FONTSMOOTHINGCLEARTYPE, SPI_GETFONTSMOOTHING, SPI_GETFONTSMOOTHINGTYPE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, Interface};

use super::SubpixelOrder;
use super::gamma::TextContrast;
use super::glyph::{
    GlyphFontId, GlyphRenderMode, GlyphSynthesis, GlyphVariationId, size_from_bits,
};
use super::raster::{
    GlyphImage, GlyphImageFormat, GlyphRasterRequest, GlyphRasterizer, SwashGlyphRasterizer,
    encode_subpixel,
};

/// DirectWrite for outline glyphs, swash for the rest.
pub(super) struct DWriteGlyphRasterizer {
    swash: SwashGlyphRasterizer,
    /// `None` when DirectWrite could not be set up at all; every glyph then
    /// goes to swash.
    dwrite: Option<DWrite>,
}

struct DWrite {
    factory: IDWriteFactory5,
    loader: IDWriteInMemoryFontFileLoader,
    params: IDWriteRenderingParams,
    /// One in-memory file per face, so the bytes are copied to DirectWrite
    /// once however many instances of the face are drawn.
    files: HashMap<nana_text::FontId, Option<IDWriteFontFile>>,
    /// A face instance DirectWrite can draw, or `None` for one it cannot, so
    /// that is asked once.
    faces: HashMap<(GlyphFontId, GlyphVariationId, GlyphSynthesis), Option<IDWriteFontFace>>,
}

impl DWriteGlyphRasterizer {
    pub(super) fn new(engine: nana_text::SharedTextEngine) -> Self {
        Self {
            swash: SwashGlyphRasterizer::new(engine),
            dwrite: DWrite::new(),
        }
    }

    pub(super) fn intern_instance(
        &mut self,
        instance: &FontInstanceKey,
    ) -> (GlyphFontId, GlyphVariationId, GlyphSynthesis) {
        self.swash.intern_instance(instance)
    }

    #[cfg(test)]
    pub(super) fn variation_count(&self) -> usize {
        self.swash.variation_count()
    }

    /// See [`SwashGlyphRasterizer::begin_frame`]. The DirectWrite faces
    /// built for the evicted ids go with them.
    pub(super) fn begin_frame(&mut self) -> Vec<GlyphVariationId> {
        let evicted = self.swash.begin_frame();
        if !evicted.is_empty()
            && let Some(dwrite) = self.dwrite.as_mut()
        {
            let gone: std::collections::HashSet<GlyphVariationId> =
                evicted.iter().copied().collect();
            dwrite
                .faces
                .retain(|(_, variation, _), _| !gone.contains(variation));
        }
        evicted
    }
}

impl GlyphRasterizer for DWriteGlyphRasterizer {
    fn rasterize(&mut self, request: &GlyphRasterRequest) -> Option<GlyphImage> {
        let key = &request.key;
        // What DirectWrite is not asked for: a turned glyph (swash applies
        // the quarter turn to the outline) and a pixel font's whole-pixel
        // strikes.
        let delegated = key.synthesis.contains(GlyphSynthesis::ROTATE_CW)
            || key.synthesis.contains(GlyphSynthesis::PIXEL_FONT)
            || key.synthesis.contains(GlyphSynthesis::DISABLE_HINTING);
        if !delegated
            && let Some(dwrite) = self.dwrite.as_mut()
            && let Some(image) = dwrite.rasterize(&self.swash, request)
        {
            return Some(image);
        }
        self.swash.rasterize(request)
    }
}

impl DWrite {
    fn new() -> Option<Self> {
        // SAFETY: plain COM factory calls; every out-parameter is owned by
        // the returned wrappers.
        unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let loader = factory.CreateInMemoryFontFileLoader().ok()?;
            factory.RegisterFontFileLoader(&loader).ok()?;
            let params = factory.CreateRenderingParams().ok()?;
            Some(Self {
                factory,
                loader,
                params,
                files: HashMap::new(),
                faces: HashMap::new(),
            })
        }
    }

    fn rasterize(
        &mut self,
        swash: &SwashGlyphRasterizer,
        request: &GlyphRasterRequest,
    ) -> Option<GlyphImage> {
        let key = &request.key;
        let face = self.face(swash, key.font, key.variation, key.synthesis)?;
        let glyph = u16::try_from(key.glyph).ok()?;
        let size = size_from_bits(key.size_bits);
        let subpixel = matches!(
            key.mode,
            GlyphRenderMode::SubpixelRgb | GlyphRenderMode::SubpixelBgr
        );
        // SAFETY: the glyph run borrows `glyph`, `advance` and `offset` for
        // the duration of `CreateGlyphRunAnalysis`, which copies what it
        // needs; the face reference it holds is released right after.
        unsafe {
            let (mode, grid_fit) = self.rendering_mode(&face, size)?;
            let advance = 0.0f32;
            let offset = DWRITE_GLYPH_OFFSET::default();
            let mut run = DWRITE_GLYPH_RUN {
                fontFace: ManuallyDrop::new(Some(face)),
                fontEmSize: size,
                glyphCount: 1,
                glyphIndices: &glyph,
                glyphAdvances: &advance,
                glyphOffsets: &offset,
                isSideways: BOOL(0),
                bidiLevel: 0,
            };
            let analysis = self.factory.CreateGlyphRunAnalysis(
                &run,
                None,
                mode,
                DWRITE_MEASURING_MODE_NATURAL,
                grid_fit,
                if subpixel {
                    DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE
                } else {
                    DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE
                },
                key.subpixel_x.as_float(),
                key.subpixel_y.as_float(),
            );
            ManuallyDrop::drop(&mut run.fontFace);
            let analysis = analysis.ok()?;
            // ClearType answers in three coverages a pixel, grayscale in one.
            // The single-coverage texture is `ALIASED_1x1`, whose name only
            // ever fitted the bi-level mode it was introduced for: asking a
            // grayscale analysis for the 3x1 texture bounds gets an empty
            // rectangle back, and with it a glyph with no ink at all.
            let texture = if subpixel {
                DWRITE_TEXTURE_CLEARTYPE_3x1
            } else {
                DWRITE_TEXTURE_ALIASED_1x1
            };
            let bounds = analysis.GetAlphaTextureBounds(texture).ok()?;
            let width = u32::try_from(bounds.right - bounds.left).unwrap_or(0);
            let height = u32::try_from(bounds.bottom - bounds.top).unwrap_or(0);
            if width == 0 || height == 0 {
                return Some(GlyphImage {
                    format: GlyphImageFormat::Mask,
                    width: 0,
                    height: 0,
                    left: 0,
                    top: 0,
                    data: Vec::new(),
                });
            }
            let pixels = width as usize * height as usize;
            let mut coverage = vec![0u8; pixels * if subpixel { 3 } else { 1 }];
            analysis
                .CreateAlphaTexture(texture, &bounds, &mut coverage)
                .ok()?;
            let (format, data) = if subpixel {
                let mut data = vec![0u8; pixels * 4];
                for (out, texel) in data
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(coverage.as_chunks::<3>().0)
                {
                    encode_subpixel(out, [texel[0], texel[1], texel[2]], key.mode);
                }
                (GlyphImageFormat::SubpixelRgb, data)
            } else {
                (GlyphImageFormat::Mask, coverage)
            };
            Some(GlyphImage {
                format,
                width,
                height,
                // DirectWrite's bounds are y-down from the baseline origin;
                // the rasterizer convention is the bitmap's top above the pen.
                left: bounds.left,
                top: -bounds.top,
                data,
            })
        }
    }

    /// What DirectWrite recommends for this face at this size, kept to the one
    /// mode that positions glyphs at fractional pixels and smooths both axes.
    /// The GDI-compatible and aliased modes snap to whole pixels, which the
    /// quarter-pixel bins this glyph was keyed by cannot follow.
    ///
    /// `None` past the outline threshold, where DirectWrite recommends
    /// bypassing its rasterizer for the outlines themselves: an analysis in
    /// that mode has no alpha texture to ask for, and asking anyway gets an
    /// empty rectangle back that is indistinguishable from a blank glyph. Such
    /// a size belongs to swash, which scales an outline however large it is.
    unsafe fn rendering_mode(
        &self,
        face: &IDWriteFontFace,
        size: f32,
    ) -> Option<(DWRITE_RENDERING_MODE1, DWRITE_GRID_FIT_MODE)> {
        let mut mode = DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC;
        let mut grid_fit = DWRITE_GRID_FIT_MODE_DEFAULT;
        if let Ok(face) = face.cast::<IDWriteFontFace3>() {
            let _ = unsafe {
                face.GetRecommendedRenderingMode(
                    size,
                    96.0,
                    96.0,
                    None,
                    false,
                    DWRITE_OUTLINE_THRESHOLD_ANTIALIASED,
                    DWRITE_MEASURING_MODE_NATURAL,
                    &self.params,
                    &mut mode,
                    &mut grid_fit,
                )
            };
        }
        if mode == DWRITE_RENDERING_MODE1_OUTLINE {
            return None;
        }
        Some((DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC, grid_fit))
    }

    fn face(
        &mut self,
        swash: &SwashGlyphRasterizer,
        font: GlyphFontId,
        variation: GlyphVariationId,
        synthesis: GlyphSynthesis,
    ) -> Option<IDWriteFontFace> {
        let key = (font, variation, synthesis);
        if let Some(face) = self.faces.get(&key) {
            return face.clone();
        }
        let face = self.create_face(swash, font, swash.coords(variation), synthesis);
        self.faces.insert(key, face.clone());
        face
    }

    fn create_face(
        &mut self,
        swash: &SwashGlyphRasterizer,
        font: GlyphFontId,
        coords: &[AxisCoord],
        synthesis: GlyphSynthesis,
    ) -> Option<IDWriteFontFace> {
        let (id, data) = swash.face_data(font)?;
        let Self {
            factory,
            loader,
            files,
            ..
        } = self;
        let file = files
            .entry(id)
            .or_insert_with(|| {
                let bytes = data.bytes();
                // SAFETY: with no owner object the loader copies the bytes, so
                // nothing here has to outlive the call.
                unsafe {
                    loader
                        .CreateInMemoryFontFileReference(
                            &*factory,
                            bytes.as_ptr().cast(),
                            u32::try_from(bytes.len()).ok()?,
                            None,
                        )
                        .ok()
                }
            })
            .clone()?;
        let mut simulations = DWRITE_FONT_SIMULATIONS_NONE;
        if synthesis.contains(GlyphSynthesis::FAKE_BOLD) {
            simulations |= DWRITE_FONT_SIMULATIONS_BOLD;
        }
        if synthesis.contains(GlyphSynthesis::FAKE_ITALIC) {
            simulations |= DWRITE_FONT_SIMULATIONS_OBLIQUE;
        }
        // SAFETY: COM calls on live interfaces; the slices outlive each call.
        unsafe {
            let mut supported = BOOL(0);
            let mut file_type = DWRITE_FONT_FILE_TYPE::default();
            let mut face_type = DWRITE_FONT_FACE_TYPE_UNKNOWN;
            let mut count = 0u32;
            file.Analyze(
                &mut supported,
                &mut file_type,
                Some(&mut face_type),
                &mut count,
            )
            .ok()?;
            // A bitmap-only face has nothing to grid-fit; swash places its
            // strikes on whole pixels.
            if !supported.as_bool() || face_type == DWRITE_FONT_FACE_TYPE_BITMAP {
                return None;
            }
            let face = if coords.is_empty() {
                self.factory
                    .CreateFontFace(face_type, &[Some(file)], data.index(), simulations)
                    .ok()?
            } else {
                let values: Vec<DWRITE_FONT_AXIS_VALUE> = coords
                    .iter()
                    .map(|coord| DWRITE_FONT_AXIS_VALUE {
                        axisTag: DWRITE_FONT_AXIS_TAG(u32::from_le_bytes(coord.tag)),
                        value: coord.value,
                    })
                    .collect();
                // Without IDWriteFactory6 (before Windows 10 1803) a varied
                // instance goes to swash rather than drawing the default one.
                self.factory
                    .cast::<IDWriteFactory6>()
                    .ok()?
                    .CreateFontResource(&file, data.index())
                    .ok()?
                    .CreateFontFace(simulations, &values)
                    .ok()?
                    .cast::<IDWriteFontFace>()
                    .ok()?
            };
            // Color glyphs need their layers painted; swash does that.
            if face
                .cast::<IDWriteFontFace2>()
                .is_ok_and(|face| face.IsColorFont().as_bool())
            {
                return None;
            }
            Some(face)
        }
    }
}

impl Drop for DWrite {
    fn drop(&mut self) {
        // The factory is shared process-wide; a loader left registered would
        // outlive this painter.
        // SAFETY: the loader was registered with this factory in `new`.
        unsafe {
            let _ = self.factory.UnregisterFontFileLoader(&self.loader);
        }
    }
}

/// The system's text rendering parameters: DirectWrite's gamma and its two
/// enhanced contrasts, as the ClearType Text Tuner last left them.
pub(super) fn system_contrast() -> Option<TextContrast> {
    // SAFETY: plain COM calls.
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let params = factory.CreateRenderingParams().ok()?;
        let grayscale = params
            .cast::<IDWriteRenderingParams1>()
            .map_or(TextContrast::DEFAULT.grayscale_contrast, |params| {
                params.GetGrayscaleEnhancedContrast()
            });
        Some(TextContrast {
            gamma: params.GetGamma(),
            grayscale_contrast: grayscale,
            cleartype_contrast: params.GetEnhancedContrast(),
        })
    }
}

/// The primary panel's subpixel order when the user has ClearType on, `None`
/// when font smoothing is off, grayscale, or the panel reports no order.
pub(super) fn system_subpixel_order() -> Option<SubpixelOrder> {
    // SAFETY: each call writes one value of the documented type.
    unsafe {
        let mut smoothing = BOOL(0);
        SystemParametersInfoW(
            SPI_GETFONTSMOOTHING,
            0,
            Some((&raw mut smoothing).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
        let mut kind = 0u32;
        SystemParametersInfoW(
            SPI_GETFONTSMOOTHINGTYPE,
            0,
            Some((&raw mut kind).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
        if !smoothing.as_bool() || kind != FE_FONTSMOOTHINGCLEARTYPE {
            return None;
        }
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        match factory.CreateRenderingParams().ok()?.GetPixelGeometry() {
            DWRITE_PIXEL_GEOMETRY_RGB => Some(SubpixelOrder::Rgb),
            DWRITE_PIXEL_GEOMETRY_BGR => Some(SubpixelOrder::Bgr),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::glyph::{GlyphRasterKey, SubpixelBin, size_bits};
    use super::*;
    use nana_text::font::Synthesis;

    /// A key for glyph `glyph` of the first face that DirectWrite accepts,
    /// with the rasterizer that interned it.
    fn rasterizer_and_key(
        glyph: u32,
        mode: GlyphRenderMode,
    ) -> Option<(DWriteGlyphRasterizer, GlyphRasterKey)> {
        let engine = crate::text_engine::nana_text_engine();
        let faces = {
            let engine = crate::text_engine::lock_engine(&engine);
            engine.fonts().faces()
        };
        let mut rasterizer = DWriteGlyphRasterizer::new(engine);
        rasterizer.dwrite.as_ref()?;
        for face in faces {
            let (font, variation, synthesis) = rasterizer.intern_instance(&FontInstanceKey {
                font: face,
                coords: Vec::new().into(),
                synthesis: Synthesis::default(),
            });
            let key = GlyphRasterKey {
                font,
                font_generation: 0,
                variation,
                glyph,
                size_bits: size_bits(16.0),
                subpixel_x: SubpixelBin::default(),
                subpixel_y: SubpixelBin::default(),
                synthesis,
                mode,
            };
            let swash = &rasterizer.swash;
            if rasterizer
                .dwrite
                .as_mut()?
                .face(swash, font, variation, synthesis)
                .is_some()
            {
                return Some((rasterizer, key));
            }
        }
        None
    }

    fn draw(rasterizer: &mut DWriteGlyphRasterizer, key: GlyphRasterKey) -> GlyphImage {
        let swash = &rasterizer.swash;
        rasterizer
            .dwrite
            .as_mut()
            .expect("set up above")
            .rasterize(swash, &GlyphRasterRequest { key })
            .expect("DirectWrite draws an outline face")
    }

    /// Some glyph with ink: glyph ids 1.. are letters in any text face.
    const GLYPH: u32 = 36;

    #[test]
    fn grayscale_glyphs_come_back_as_one_coverage_a_pixel() {
        let Some((mut rasterizer, key)) = rasterizer_and_key(GLYPH, GlyphRenderMode::Mask) else {
            return;
        };
        let image = draw(&mut rasterizer, key);
        assert_eq!(image.format, GlyphImageFormat::Mask);
        assert_eq!(image.data.len(), (image.width * image.height) as usize);
        assert!(
            image.data.iter().any(|&c| c > 200),
            "the glyph has solid ink"
        );
    }

    #[test]
    fn cleartype_glyphs_carry_distinct_subpixel_coverage_mirrored_for_bgr() {
        let Some((mut rasterizer, rgb_key)) =
            rasterizer_and_key(GLYPH, GlyphRenderMode::SubpixelRgb)
        else {
            return;
        };
        let rgb = draw(&mut rasterizer, rgb_key);
        let bgr = draw(
            &mut rasterizer,
            GlyphRasterKey {
                mode: GlyphRenderMode::SubpixelBgr,
                ..rgb_key
            },
        );
        assert_eq!(rgb.format, GlyphImageFormat::SubpixelRgb);
        assert!(
            rgb.data.as_chunks::<4>().0.iter().any(|p| p[0] != p[2]),
            "stem edges differ per subpixel"
        );
        for (a, b) in rgb
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(bgr.data.as_chunks::<4>().0.iter())
        {
            assert_eq!([a[0], a[1], a[2], a[3]], [b[2], b[1], b[0], b[3]]);
        }
    }

    #[test]
    fn a_quarter_pixel_shift_changes_the_bitmap() {
        let Some((mut rasterizer, key)) = rasterizer_and_key(GLYPH, GlyphRenderMode::Mask) else {
            return;
        };
        let whole = draw(&mut rasterizer, key);
        let (_, half) = SubpixelBin::split(0.5);
        let shifted = draw(
            &mut rasterizer,
            GlyphRasterKey {
                subpixel_x: half,
                ..key
            },
        );
        assert_ne!(
            whole.data, shifted.data,
            "the pen offset reaches DirectWrite"
        );
    }

    #[test]
    fn a_glyph_past_the_outline_threshold_still_comes_back_with_ink() {
        let Some((mut rasterizer, key)) = rasterizer_and_key(GLYPH, GlyphRenderMode::Mask) else {
            return;
        };
        // Above its outline threshold DirectWrite recommends bypassing the
        // rasterizer, and an analysis built that way has no alpha texture at
        // all. The glyph has to reach swash rather than come back blank.
        let image = rasterizer
            .rasterize(&GlyphRasterRequest {
                key: GlyphRasterKey {
                    size_bits: size_bits(400.0),
                    ..key
                },
            })
            .expect("a glyph this large still rasterizes");
        assert!(
            image.data.iter().any(|&c| c > 200),
            "the glyph has solid ink"
        );
    }

    #[test]
    fn the_system_parameters_are_in_directwrite_range() {
        let Some(contrast) = system_contrast() else {
            return;
        };
        assert!(contrast.gamma >= 1.0, "{contrast:?}");
        assert!(contrast.grayscale_contrast >= 0.0 && contrast.cleartype_contrast >= 0.0);
    }
}
