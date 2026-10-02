// Adapted from historical Iced (MIT).
struct Globals {
    transform: mat4x4<f32>,
    // Logical → physical scale (`ScenePaintViewport.scale_factor`). Combined
    // with instance affine σ_min so the covering quad contains the AA band.
    viewport_scale: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
