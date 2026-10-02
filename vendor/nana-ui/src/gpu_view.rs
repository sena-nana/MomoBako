//! Physical scissor math and the demo `"gpu-view"` WGSL.
//!
//! The product [`nana_ui_runtime::GpuView`] type lives in `nana-ui-runtime`.
//! This module does not define that component.

use crate::geometry::{LogicalRect, PhysicalRect};

pub(crate) const GPU_VIEW_SHADER: &str = r#"
struct Instance {
    // Destination rect in physical pixels, unclipped. This is the region the
    // pattern is mapped over, so a partially clipped node keeps its mapping.
    // Ancestor clipping stays on the pass scissor: a run shares one clip.
    @location(0) rect: vec4<f32>,
    @location(1) color_a: vec4<f32>,
    @location(2) color_b: vec4<f32>,
    // x = seed, zw = dest size in physical pixels. The dest size rides the
    // instance so a run needs no bound uniform and no per-frame upload.
    @location(3) parameters: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color_a: vec4<f32>,
    @location(2) color_b: vec4<f32>,
    @location(3) parameters: vec4<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    let corner = corners[index];
    let pixel = instance.rect.xy + corner * instance.rect.zw;
    let size = max(instance.parameters.zw, vec2<f32>(1.0));
    var output: VertexOutput;
    output.position = vec4<f32>(
        pixel.x / size.x * 2.0 - 1.0,
        1.0 - pixel.y / size.y * 2.0,
        0.0,
        1.0,
    );
    // Matches the viewport-mapped fullscreen triangle this shader replaced:
    // uv.y runs from 0 at the bottom edge to 1 at the top.
    output.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    output.color_a = instance.color_a;
    output.color_b = instance.color_b;
    output.parameters = instance.parameters;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let seed = input.parameters.x;
    let wave = 0.5 + 0.5 * sin((uv.x * 5.5 + uv.y * 3.0 + seed) * 3.14159265);
    let radial = smoothstep(0.82, 0.05, distance(uv, vec2<f32>(0.64, 0.42)));
    let grid_x = 1.0 - smoothstep(0.0, 0.035, abs(fract(uv.x * 12.0) - 0.5));
    let grid_y = 1.0 - smoothstep(0.0, 0.035, abs(fract(uv.y * 8.0) - 0.5));
    let grid = max(grid_x, grid_y) * 0.08;
    let mix_amount = clamp(0.18 + wave * 0.34 + radial * 0.28, 0.0, 1.0);
    let color = mix(input.color_a.rgb, input.color_b.rgb, mix_amount) + grid;
    return vec4<f32>(color, 1.0);
}
"#;

/// A stable logical/physical region suitable for a viewport and scissor.
///
/// Used by the Scene painter. Distinct from any Runtime GPU-slot helper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSlot {
    pub id: u64,
    pub logical: LogicalRect,
    pub physical: PhysicalRect,
}

impl RenderSlot {
    pub fn new(id: u64, logical: LogicalRect, scale_factor: f32) -> Self {
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let left = (logical.x * scale_factor).floor().max(0.0);
        let top = (logical.y * scale_factor).floor().max(0.0);
        let right = ((logical.x + logical.width) * scale_factor)
            .ceil()
            .max(left);
        let bottom = ((logical.y + logical.height) * scale_factor)
            .ceil()
            .max(top);

        Self {
            id,
            logical,
            physical: PhysicalRect {
                x: saturating_u32(left),
                y: saturating_u32(top),
                width: saturating_u32(right - left),
                height: saturating_u32(bottom - top),
            },
        }
    }

    pub fn clipped_physical(self, target_width: u32, target_height: u32) -> PhysicalRect {
        let right = self
            .physical
            .x
            .saturating_add(self.physical.width)
            .min(target_width);
        let bottom = self
            .physical
            .y
            .saturating_add(self.physical.height)
            .min(target_height);
        let x = self.physical.x.min(target_width);
        let y = self.physical.y.min(target_height);

        PhysicalRect {
            x,
            y,
            width: right.saturating_sub(x),
            height: bottom.saturating_sub(y),
        }
    }
}

fn saturating_u32(value: f32) -> u32 {
    value.clamp(0.0, u32::MAX as f32) as u32
}

pub(crate) fn slot_for_bounds(
    id: u64,
    bounds: LogicalRect,
    scale_factor: f32,
) -> (RenderSlot, [f32; 4]) {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let slot = RenderSlot::new(id, bounds, scale_factor);

    (
        slot,
        [
            bounds.x * scale_factor,
            bounds.y * scale_factor,
            bounds.width * scale_factor,
            bounds.height * scale_factor,
        ],
    )
}

pub(crate) fn intersect_physical(slot: PhysicalRect, clip: PhysicalRect) -> PhysicalRect {
    let left = slot.x.max(clip.x);
    let top = slot.y.max(clip.y);
    let right = slot
        .x
        .saturating_add(slot.width)
        .min(clip.x.saturating_add(clip.width));
    let bottom = slot
        .y
        .saturating_add(slot.height)
        .min(clip.y.saturating_add(clip.height));

    PhysicalRect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderSlot, intersect_physical};
    use crate::geometry::{LogicalRect, PhysicalRect};

    #[test]
    fn render_slot_covers_fractional_physical_edges_and_clips_to_target() {
        let slot = RenderSlot::new(7, LogicalRect::new(10.25, 20.5, 100.5, 50.25), 1.5);
        assert_eq!(slot.physical.x, 15);
        assert_eq!(slot.physical.y, 30);
        assert_eq!(slot.physical.width, 152);
        assert_eq!(slot.physical.height, 77);

        let clipped = slot.clipped_physical(120, 90);
        assert_eq!(clipped.x, 15);
        assert_eq!(clipped.y, 30);
        assert_eq!(clipped.width, 105);
        assert_eq!(clipped.height, 60);
    }

    #[test]
    fn render_slot_sanitizes_scale_and_never_underflows_when_outside_target() {
        let slot = RenderSlot::new(1, LogicalRect::new(50.0, 60.0, 20.0, 30.0), f32::NAN);
        assert_eq!(slot.physical.width, 20);
        assert_eq!(slot.physical.height, 30);
        assert_eq!(
            slot.clipped_physical(10, 10),
            PhysicalRect {
                x: 10,
                y: 10,
                width: 0,
                height: 0,
            }
        );
    }

    #[test]
    fn physical_intersection_rejects_clipped_and_overlapping_edges() {
        assert_eq!(
            intersect_physical(
                PhysicalRect {
                    x: 10,
                    y: 20,
                    width: 40,
                    height: 30,
                },
                PhysicalRect {
                    x: 20,
                    y: 25,
                    width: 15,
                    height: 10,
                },
            ),
            PhysicalRect {
                x: 20,
                y: 25,
                width: 15,
                height: 10,
            }
        );
    }
}
