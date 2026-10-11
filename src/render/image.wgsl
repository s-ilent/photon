// Image canvas shader: one tile per draw, tinted while loading.

struct Globals {
    viewport_px: vec2<f32>,
    viewport_size_px: vec2<f32>,
    screen_size_px: vec2<f32>,
    tint: vec4<f32>,
    sharp: f32,         // > 0.5 = pixel-sharp
    linear_source: f32, // > 0.5 = linear-light source
    hdr_source: f32,    // > 0.5 = float/HDR source, tonemap
    exposure: f32,      // exposure in EV (only when hdr_source)
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

@group(1) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) uv: vec2<f32>) -> VsOut {
    // Absolute pixels -> NDC; `paint` overrides the viewport to the full target.
    let px = globals.viewport_px + uv * globals.viewport_size_px;
    let ndc = vec2<f32>(
        2.0 * px.x / globals.screen_size_px.x - 1.0,
        1.0 - 2.0 * px.y / globals.screen_size_px.y,
    );
    return VsOut(vec4<f32>(ndc, 0.0, 1.0), uv);
}

// Snaps the sample to the texel centre; box from fwidth so it holds at any zoom.
fn sharp_sample_uv(uv: vec2<f32>) -> vec2<f32> {
    let tex_size = vec2<f32>(textureDimensions(tex)); // (w, h)
    let inv_size = 1.0 / tex_size;                    // (1/w, 1/h)
    // This fragment's footprint in source pixels, clamped to (0, 1].
    let box = clamp(fwidth(uv) * tex_size, vec2<f32>(1e-5), vec2<f32>(1.0));
    let coord = uv * tex_size - 0.5 * box;
    let tx_offset = smoothstep(vec2<f32>(1.0) - box, vec2<f32>(1.0), fract(coord));
    return (floor(coord) + vec2<f32>(0.5) + tx_offset) * inv_size;
}

// Linear-light -> display sRGB encode, mirroring egui's gamma_from_linear_rgb.
// max() guards pow() because both select arms evaluate.
fn gamma_from_linear_rgb(rgb: vec3<f32>) -> vec3<f32> {
    let hi = 1.055 * pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    let lo = rgb * 12.92;
    return select(hi, lo, rgb <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // `select` evaluates both arms, so the derivative in `sharp_sample_uv`
    // always runs in uniform control flow.
    let uv = select(in.uv, sharp_sample_uv(in.uv), globals.sharp > 0.5);
    let sampled = textureSample(tex, linear_sampler, uv);
    // HDR: scale by exposure, then Reinhard; display-encode a linear source;
    // tint stays in display space to match egui's gamma-space pass. Alpha is
    // neither tonemapped nor encoded.
    var rgb = sampled.rgb;
    if (globals.hdr_source > 0.5) {
        let scaled = max(rgb, vec3<f32>(0.0)) * exp2(globals.exposure);
        rgb = scaled / (vec3<f32>(1.0) + scaled);
    }
    if (globals.linear_source > 0.5) {
        rgb = gamma_from_linear_rgb(rgb);
    }
    return vec4<f32>(rgb * globals.tint.rgb, sampled.a * globals.tint.a);
}
