//! How glyph coverage becomes alpha.
//!
//! The target is an sRGB format, so the blend unit mixes in linear light. A
//! rasterizer's coverage is geometric, and applied as linear alpha it makes a
//! dark stem on a light ground come out far lighter than the native renderers
//! draw it: a half-covered edge pixel of black-on-white lands at sRGB 0.74,
//! so strokes thin out and their edges read as grey haze. DirectWrite, Skia
//! and CoreText all blend text in gamma space and then correct the coverage
//! for the foreground's brightness.
//!
//! This reproduces DirectWrite's grayscale correction (enhanced contrast, then
//! the gamma alpha correction) and then works out the coverage a *linear*
//! blend needs to land on the color DirectWrite's gamma-space blend would have
//! produced. The background that needs is not known per pixel, so it is taken
//! to be the foreground's opposite — Skia's guess, which is exact for dark text
//! on light and light text on dark and has no discontinuity in between.
//!
//! [`TextContrast::coverage`] is the CPU statement of what the shader does,
//! written out so the correction can be tested without a GPU.

/// DirectWrite's alpha correction coefficients, one row per 0.1 of gamma from
/// 1.0 to 2.2. Ported from Windows Terminal's AtlasEngine
/// (`src/renderer/atlas/dwrite.cpp`), Copyright (c) Microsoft Corporation,
/// MIT. See `docs/reference/third-party.md`.
const GAMMA_INCORRECT_TARGET_RATIOS: [[f32; 4]; 13] = [
    [0.0000, 0.0000, 0.0000, 0.0000],
    [0.0166, -0.0807, 0.2227, -0.0751],
    [0.0350, -0.1760, 0.4325, -0.1370],
    [0.0543, -0.2821, 0.6302, -0.1876],
    [0.0739, -0.3963, 0.8167, -0.2287],
    [0.0933, -0.5161, 0.9926, -0.2616],
    [0.1121, -0.6395, 1.1588, -0.2877],
    [0.1300, -0.7649, 1.3159, -0.3080],
    [0.1469, -0.8911, 1.4644, -0.3234],
    [0.1627, -1.0170, 1.6051, -0.3347],
    [0.1773, -1.1420, 1.7385, -0.3426],
    [0.1908, -1.2652, 1.8650, -0.3476],
    [0.2031, -1.3864, 1.9851, -0.3501],
];

/// What a platform's text rendering parameters decide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TextContrast {
    /// DirectWrite's gamma, 1.0..=2.2.
    pub gamma: f32,
    /// DirectWrite's grayscale enhanced contrast, 0 upward.
    pub grayscale_contrast: f32,
    /// DirectWrite's (ClearType) enhanced contrast, 0 upward.
    pub cleartype_contrast: f32,
}

impl TextContrast {
    /// DirectWrite's own defaults (`IDWriteFactory::CreateRenderingParams`
    /// on an untuned system): gamma 1.8, grayscale enhanced contrast 1.0,
    /// ClearType enhanced contrast 0.5.
    pub(super) const DEFAULT: Self = Self {
        gamma: 1.8,
        grayscale_contrast: 1.0,
        cleartype_contrast: 0.5,
    };

    /// This platform's parameters.
    pub(super) fn system() -> Self {
        #[cfg(windows)]
        if let Some(contrast) = super::raster_dwrite::system_contrast() {
            return contrast;
        }
        Self::DEFAULT
    }

    /// The alpha correction coefficients for this gamma, pre-scaled the way
    /// DirectWrite scales them for coverage and color in 0..=1.
    pub(super) fn gamma_ratios(self) -> [f32; 4] {
        let norm13 = (f64::from(0x1_0000) / (255.0 * 255.0)) as f32;
        let norm24 = (f64::from(0x100) / 255.0) as f32;
        let index = ((self.gamma.clamp(1.0, 2.2) - 1.0) * 10.0).round() as usize;
        let row = GAMMA_INCORRECT_TARGET_RATIOS[index];
        [
            norm13 * row[0],
            norm24 * row[1],
            norm13 * row[2],
            norm24 * row[3],
        ]
    }

    /// What the text shader's globals carry: the ratios, then the contrast.
    pub(super) fn to_gpu(self) -> [f32; 8] {
        let [a, b, c, d] = self.gamma_ratios();
        [
            a,
            b,
            c,
            d,
            self.grayscale_contrast,
            self.cleartype_contrast,
            0.0,
            0.0,
        ]
    }

    /// The alpha a glyph pixel of `coverage` paints with, for a foreground
    /// whose straight color is `linear` (linear RGB).
    ///
    /// The shader's `corrected_coverage`, restated.
    #[cfg(test)]
    pub(super) fn coverage(self, coverage: f32, linear: [f32; 3]) -> f32 {
        let encoded = linear.map(linear_to_srgb);
        let brightness = encoded[0] * 0.30 + encoded[1] * 0.59 + encoded[2] * 0.11;
        let intensity = encoded[0] * 0.25 + encoded[1] * 0.5 + encoded[2] * 0.25;
        let k = self.grayscale_contrast * (4.0 * (0.75 - brightness)).clamp(0.0, 1.0);
        let g = self.gamma_ratios();
        let a = coverage * (k + 1.0) / (coverage * k + 1.0);
        let a = a + a * (1.0 - a) * ((g[0] * intensity + g[1]) * a + (g[2] * intensity + g[3]));
        let src = encoded[0] * 0.2126 + encoded[1] * 0.7152 + encoded[2] * 0.0722;
        let dst = 1.0 - src;
        if (src - dst).abs() < 1.0 / 256.0 {
            return a;
        }
        let out = srgb_to_linear(dst + (src - dst) * a);
        let (src, dst) = (srgb_to_linear(src), srgb_to_linear(dst));
        ((out - dst) / (src - dst)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the screen shows for `alpha` of `fg` over `bg` in a linear blend,
    /// sRGB-encoded.
    fn shown(alpha: f32, fg: f32, bg: f32) -> f32 {
        linear_to_srgb(srgb_to_linear(bg) + (srgb_to_linear(fg) - srgb_to_linear(bg)) * alpha)
    }

    #[test]
    fn dark_text_on_light_is_no_longer_washed_out() {
        let contrast = TextContrast::DEFAULT;
        let raw = shown(0.5, 0.0, 1.0);
        let corrected = shown(contrast.coverage(0.5, [0.0; 3]), 0.0, 1.0);
        assert!(raw > 0.7, "the uncorrected edge is the grey haze: {raw}");
        assert!(
            corrected < 0.6,
            "a half-covered black-on-white pixel reads as mid-ink: {corrected}"
        );
    }

    #[test]
    fn light_text_on_dark_is_not_made_heavier_than_a_linear_blend() {
        let contrast = TextContrast::DEFAULT;
        let corrected = contrast.coverage(0.5, [1.0; 3]);
        assert!(
            corrected <= 0.5,
            "light-on-dark already blooms in a linear blend: {corrected}"
        );
    }

    #[test]
    fn empty_and_full_coverage_stay_put() {
        let contrast = TextContrast::DEFAULT;
        for color in [[0.0; 3], [1.0; 3], [0.2, 0.4, 0.8], [0.214; 3]] {
            assert!(contrast.coverage(0.0, color).abs() < 1e-6);
            assert!((contrast.coverage(1.0, color) - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn coverage_stays_monotonic() {
        let contrast = TextContrast::DEFAULT;
        for color in [[0.0; 3], [1.0; 3], [0.05, 0.3, 0.9], [0.214; 3]] {
            let mut last = 0.0;
            for step in 0..=255 {
                let alpha = contrast.coverage(step as f32 / 255.0, color);
                assert!(alpha + 1e-6 >= last, "{color:?} at {step}");
                last = alpha;
            }
        }
    }

    #[test]
    fn gamma_one_without_contrast_is_the_identity_in_gamma_space() {
        let flat = TextContrast {
            gamma: 1.0,
            grayscale_contrast: 0.0,
            cleartype_contrast: 0.0,
        };
        // No DirectWrite correction left, so what remains is a plain
        // gamma-space blend: half coverage shows halfway in sRGB.
        let shown_half = shown(flat.coverage(0.5, [0.0; 3]), 0.0, 1.0);
        assert!((shown_half - 0.5).abs() < 1e-3, "{shown_half}");
    }
}
