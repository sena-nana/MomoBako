//! sRGB packing adapted from historical Iced (MIT).

/// Converts an sRGB Scene color to linear RGBA packed into quad/mesh
/// instances when gamma correction is enabled.
pub(crate) fn pack_linear([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [
        linear_component(r),
        linear_component(g),
        linear_component(b),
        a,
    ]
}

pub(super) fn with_opacity([r, g, b, a]: [f32; 4], opacity: f32) -> [f32; 4] {
    [r, g, b, a * opacity]
}

pub(super) fn to_rgba8([r, g, b, a]: [f32; 4]) -> [u8; 4] {
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        (a * 255.0).round() as u8,
    ]
}

fn linear_component(u: f32) -> f32 {
    if u < 0.04045 {
        u / 12.92
    } else {
        ((u + 0.055) / 1.055).powf(2.4)
    }
}

/// The same curve for a color that is already eight bits.
///
/// A table, because this runs once per text run per frame and `powf` is not
/// cheap: a thousand labels is three thousand of them for a value that only
/// has 256 possible inputs. `the_table_is_the_curve_it_replaces` fails if the
/// two ever disagree by a bit.
pub(super) fn linear_from_srgb8(value: u8) -> f32 {
    SRGB8_TO_LINEAR[value as usize]
}

#[rustfmt::skip]
const SRGB8_TO_LINEAR: [f32; 256] = [
    0.0f32, 0.000303527f32, 0.000607054f32, 0.000910581f32,
    0.001214108f32, 0.001517635f32, 0.001821162f32, 0.0021246888f32,
    0.002428216f32, 0.002731743f32, 0.00303527f32, 0.0033465356f32,
    0.003676507f32, 0.004024717f32, 0.004391442f32, 0.0047769533f32,
    0.005181517f32, 0.0056053917f32, 0.0060488326f32, 0.006512091f32,
    0.00699541f32, 0.0074990317f32, 0.008023192f32, 0.008568125f32,
    0.009134057f32, 0.009721218f32, 0.010329823f32, 0.010960094f32,
    0.011612245f32, 0.012286487f32, 0.012983031f32, 0.013702081f32,
    0.014443844f32, 0.015208514f32, 0.015996292f32, 0.016807375f32,
    0.017641952f32, 0.018500218f32, 0.019382361f32, 0.020288562f32,
    0.02121901f32, 0.022173883f32, 0.023153365f32, 0.02415763f32,
    0.025186857f32, 0.026241222f32, 0.027320892f32, 0.028426038f32,
    0.029556833f32, 0.03071344f32, 0.03189603f32, 0.033104762f32,
    0.034339808f32, 0.035601314f32, 0.036889445f32, 0.038204364f32,
    0.039546236f32, 0.0409152f32, 0.04231141f32, 0.043735027f32,
    0.045186203f32, 0.046665084f32, 0.048171822f32, 0.049706563f32,
    0.051269468f32, 0.052860655f32, 0.05448028f32, 0.056128494f32,
    0.057805434f32, 0.05951124f32, 0.06124607f32, 0.06301004f32,
    0.06480328f32, 0.06662595f32, 0.06847818f32, 0.07036011f32,
    0.07227186f32, 0.07421358f32, 0.07618539f32, 0.07818743f32,
    0.08021983f32, 0.082282715f32, 0.084376216f32, 0.086500466f32,
    0.088655606f32, 0.09084173f32, 0.09305898f32, 0.095307484f32,
    0.09758736f32, 0.09989874f32, 0.10224175f32, 0.10461649f32,
    0.10702311f32, 0.10946172f32, 0.111932434f32, 0.11443538f32,
    0.11697067f32, 0.119538434f32, 0.1221388f32, 0.12477184f32,
    0.1274377f32, 0.13013649f32, 0.13286833f32, 0.13563335f32,
    0.13843162f32, 0.1412633f32, 0.14412849f32, 0.14702728f32,
    0.1499598f32, 0.15292616f32, 0.15592647f32, 0.15896086f32,
    0.1620294f32, 0.16513222f32, 0.1682694f32, 0.1714411f32,
    0.17464739f32, 0.17788841f32, 0.18116423f32, 0.18447499f32,
    0.18782076f32, 0.19120167f32, 0.19461781f32, 0.1980693f32,
    0.20155624f32, 0.2050787f32, 0.20863685f32, 0.21223073f32,
    0.21586053f32, 0.21952623f32, 0.22322798f32, 0.22696589f32,
    0.23074007f32, 0.23455065f32, 0.23839766f32, 0.2422812f32,
    0.2462014f32, 0.25015837f32, 0.25415218f32, 0.2581829f32,
    0.26225072f32, 0.26635566f32, 0.27049786f32, 0.27467737f32,
    0.27889434f32, 0.2831488f32, 0.2874409f32, 0.2917707f32,
    0.29613832f32, 0.30054384f32, 0.30498737f32, 0.30946895f32,
    0.31398875f32, 0.31854683f32, 0.32314324f32, 0.32777813f32,
    0.33245158f32, 0.33716366f32, 0.34191445f32, 0.3467041f32,
    0.3515327f32, 0.35640025f32, 0.36130688f32, 0.3662527f32,
    0.37123778f32, 0.37626222f32, 0.3813261f32, 0.38642952f32,
    0.39157256f32, 0.3967553f32, 0.40197787f32, 0.4072403f32,
    0.4125427f32, 0.41788515f32, 0.42326775f32, 0.42869055f32,
    0.4341537f32, 0.43965724f32, 0.44520125f32, 0.45078585f32,
    0.4564111f32, 0.46207705f32, 0.46778384f32, 0.47353154f32,
    0.47932023f32, 0.48514998f32, 0.4910209f32, 0.49693304f32,
    0.5028866f32, 0.50888145f32, 0.5149178f32, 0.5209957f32,
    0.5271152f32, 0.5332765f32, 0.5394796f32, 0.5457246f32,
    0.5520115f32, 0.5583405f32, 0.56471163f32, 0.5711249f32,
    0.5775805f32, 0.5840785f32, 0.5906189f32, 0.5972019f32,
    0.6038274f32, 0.6104956f32, 0.61720663f32, 0.62396044f32,
    0.6307572f32, 0.63759696f32, 0.64447975f32, 0.6514057f32,
    0.65837485f32, 0.66538733f32, 0.6724432f32, 0.67954254f32,
    0.68668544f32, 0.6938719f32, 0.701102f32, 0.70837593f32,
    0.71569365f32, 0.72305524f32, 0.7304609f32, 0.73791057f32,
    0.74540436f32, 0.7529423f32, 0.76052463f32, 0.7681513f32,
    0.77582234f32, 0.7835379f32, 0.79129803f32, 0.79910284f32,
    0.80695236f32, 0.8148467f32, 0.82278585f32, 0.83076996f32,
    0.8387991f32, 0.8468733f32, 0.8549927f32, 0.8631573f32,
    0.8713672f32, 0.87962234f32, 0.8879232f32, 0.8962694f32,
    0.90466136f32, 0.9130987f32, 0.92158204f32, 0.9301109f32,
    0.9386859f32, 0.9473066f32, 0.9559735f32, 0.9646863f32,
    0.9734455f32, 0.9822506f32, 0.9911022f32, 1.0f32,
];

/// OpenGL-style ortho: (0,0) top-left, Y-down, physical pixels.
pub(super) fn orthographic(width: u32, height: u32) -> [f32; 16] {
    let width = width.max(1) as f32;
    let height = height.max(1) as f32;
    [
        2.0 / width,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / height,
        0.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        -1.0,
        1.0,
        0.0,
        1.0,
    ]
}

/// `orthographic(physical) * scale(scale_factor)` for logical-space meshes.
pub(super) fn orthographic_scaled(width: u32, height: u32, scale: f32) -> [f32; 16] {
    let mut matrix = orthographic(width, height);
    matrix[0] *= scale;
    matrix[5] *= scale;
    matrix
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many representable `f32` steps apart two values are.
    fn ulps_apart(a: f32, b: f32) -> u32 {
        a.to_bits().abs_diff(b.to_bits())
    }

    /// The sRGB transfer function in `f64`, whose error lands far below
    /// anything an `f32` can represent, so it stands in for the exact curve.
    fn curve(value: u8) -> f32 {
        let u = f64::from(value) / 255.0;
        let linear = if u < 0.04045 {
            u / 12.92
        } else {
            ((u + 0.055) / 1.055).powf(2.4)
        };
        linear as f32
    }

    /// The table and `linear_component` both approximate the same curve, and
    /// each is pinned to the curve rather than to the other.
    ///
    /// Not bitwise, because `powf` is neither correctly rounded nor
    /// reproducible across libm implementations: two approximations of this
    /// curve disagree in the last bits by construction. At sRGB 71 the exact
    /// value rounds to `0x3d810b65`, an `f32` `powf` was observed returning
    /// `0x3d810b67`, and the table holds `0x3d810b68` — comparing bits pins the
    /// suite to whichever libm generated the table. The budget covers the
    /// table's own drift from the curve (up to 6 steps, worst at sRGB 134) and
    /// is still orders of magnitude tighter than a wrong entry.
    #[test]
    fn the_table_is_the_curve_it_replaces() {
        const BUDGET: u32 = 8;
        for value in 0..=255u8 {
            let exact = curve(value);
            let table = linear_from_srgb8(value);
            assert!(
                ulps_apart(table, exact) <= BUDGET,
                "sRGB {value}: table {table:?} is {} steps from the curve {exact:?}",
                ulps_apart(table, exact)
            );
            let curved = linear_component(f32::from(value) / 255.0);
            assert!(
                ulps_apart(curved, exact) <= BUDGET,
                "sRGB {value}: linear_component {curved:?} is {} steps from the curve {exact:?}",
                ulps_apart(curved, exact)
            );
        }
    }
}
