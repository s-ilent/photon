// Photon image canvas shader: draws one tile of the displayed image with
// either linear or nearest sampling (sharp/pixel scaling), tinted while the
// image loads. Coordinates match egui's conventions: positions are physical
// pixels with +y down, mapped to NDC the same way egui's own pipeline does.

struct Globals {
    // Top-left of the callback viewport in physical pixels, and its size.
    viewport_px: vec2<f32>,
    viewport_size_px: vec2<f32>,
    // Full screen size in physical pixels (for the -> NDC transform).
    screen_size_px: vec2<f32>,
    tint: vec4<f32>,
    // > 0.5: sample nearest (sharp pixel scaling).
    sharp: f32,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var nearest_sampler: sampler;

@group(1) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) norm: vec2<f32>, // image-normalized quad corner, 0..1
    @location(1) uv: vec2<f32>,   // texture coordinates of this tile's corners
) -> VsOut {
    let px = globals.viewport_px + norm * globals.viewport_size_px;
    let out: VsOut = VsOut(
        vec4<f32>(
            2.0 * px.x / globals.screen_size_px.x - 1.0,
            1.0 - 2.0 * px.y / globals.screen_size_px.y,
            0.0,
            1.0,
        ),
        uv,
    );
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var color: vec4<f32>;
    if (globals.sharp > 0.5) {
        color = textureSample(tex, nearest_sampler, in.uv);
    } else {
        color = textureSample(tex, linear_sampler, in.uv);
    }
    return color * globals.tint;
}
