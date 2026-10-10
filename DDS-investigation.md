# DDS investigation notes

Notes from investigating a slow/stuck load on a large DDS file. Not current
behavior to rely on — this is a record of exploration, to be resumed later.

## The reported problem

`/mnt/twining/Development/Starbreach/Starbreach/Resources/Clouds.dds`
(402,653,184 bytes ≈ 400 MiB) appeared to hang in Photon for 5–10 minutes
with no image shown and no error surfaced.

## What the file actually is

- 2048 × 2048, **6-face cubemap** (`caps2` indicates cubemap)
- Format: `A32B32G32R32F` — 128-bit-per-pixel **uncompressed float** (D3D9 legacy
  header, no DX10 extension)
- `image_dds::image_from_dds(&dds, 0)` decodes all 6 faces **stacked vertically**
  into a single 2048 × **12288** RGBA8 image (~100 MiB of RGBA8)
- Decode itself is fast in isolation: `Dds::read` ~0.1s, `image_from_dds` ~0.1s,
  `into_raw` negligible. The minutes-long stall is **not** in the decode

## Prime suspect (unverified)

`wgpu::Limits::default().max_texture_dimension_2d` is **8192**. The stacked
cubemap image is 2048×**12288**, which **exceeds** that limit.

`device.create_texture` with an oversized extent is a validation error in wgpu.
In Photon's worker thread (`src/app/loader.rs`), `create_texture` is called
without any size check, and the error path in wgpu 29 may surface
asynchronously — the worker may have created the texture, queued the write,
sent `LoadResult::Loaded`, and only hit the validation error when the main
thread later tries to use the texture. That mismatch between "sent Loaded"
and "texture is actually invalid" is a plausible explanation for a stuck UI
with no `StatusOverlay::Error`.

**This was NOT confirmed against a live wgpu context in this pass.** An attempt
to reproduce with a standalone test harness stalled on wgpu 29 API churn
(`ExperimentalFeatures`, `PollType::Wait { submission_index, timeout }`,
`Trace`) and was abandoned per the user's request to table the work. The
standalone test app lives at `/tmp/ddstest` if you want to resume it; it needs
the wgpu 29 device-request boilerplate corrected (see "next steps").

## What would and wouldn't fix it

- **Compressed-to-VRAM path (Plan.md)** would NOT help this file. It's already
  uncompressed 128-bit float; there is no BCn compressed byte stream to skip
  decoding. You'd still upload ~400 MiB and still need a float→RGBA8 tonemap
  step somewhere (CPU now, or a custom shader later).
- **Shader + tonemap pass** (the planned future rendering path) is the real fix:
  it lets you upload `Rgba32Float` as a wgpu texture and do float→RGBA8 + gamma
  on the GPU, and it also unlocks the sharp-filtering / proper gamma goals
  the user described.
- **Interim mitigations** to explore before the shader pass lands:
  - Reject DDS textures whose stacked cubemap height exceeds
    `device.limits().max_texture_dimension_2d` and surface a `StatusOverlay`
    instead of silently sending an oversized `LoadResult::Loaded`.
  - For cubemaps, decode and display **only face 0** (drop faces 1–5 from the
    stack) — a 2048×2048 single face fits within limits and is a reasonable
    preview. We currently call `image_from_dds(&dds, 0)` which decodes mip 0
    of *all* faces into one stacked image.
  - Split the stacked image into 6 separate `Texture` objects (one per face)
    and display only face 0 (or let the user pick).

## Other DDS observations (no fix needed — display issues, not decode bugs)

### BC7 normal maps render as white / cyan / pink / red

Sample: `/mnt/twining/Windows Games/Skyrim VR/MO2New/mods/Skyland AIO/Textures/Landscape/mountains/mountainslab02_n.dds`
(8192×8192, BC7 `DX10` header).

This is **correct decode**, not a bug. BC7 stores all four channels and Skyrim
normal maps pack extra data into B/A — the file really is those colors. A
proper fix is a **normal-map preview mode** (reconstruct `B = sqrt(1 - R² - G²)`
and/or show one channel at a time), which belongs to the future shader + view
mode work. Added to Todo.md as a feature item.

## Next concrete steps for whoever resumes

1. Fix the wgpu 29 boilerplate in `/tmp/ddstest/src/main.rs`:
   - `request_device` signature: drop the `None` arg, add `experimental_features:
     <whatever wgpu 29 expects — check device.rs for the real struct/Default>`,
     `trace: Option<wgpu::Trace>` or just `trace: None`. Read the actual struct
     in `~/.cargo/registry/.../wgpu-29.0.4/src/api/device.rs` first; don't guess.
   - `device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })`
   - `push_error_scope` returns an `ErrorScopeGuard`; check how to await/poll it
     in wgpu 29 (it's not a Future directly per the errors seen).
2. Reproduce `create_texture` on a 2048×12288 RGBA8 buffer and confirm whether
   it errors, hangs, or silently produces an invalid texture.
3. Confirm `device.limits().max_texture_dimension_2d` on the actual machine.
4. If the limit is the cause, add a size guard in `src/app/loader.rs` worker
   path and in `check_load_complete` — plus consider face 0-only cubemap decode
   in `src/image_loader.rs::load_dds` as the short-term mitigation.

## Relevant source references

- `src/app/loader.rs:128` — `device.create_texture(...)` with no size check
- `src/app/loader.rs:143` — `queue.write_texture(...)`
- `src/app/loader.rs:180` — `res_tx.send(LoadResult::Loaded { ..., view, ... })`
  (the view may be backed by an invalid texture when oversized)
- `src/image_loader.rs::load_dds` — where we could split/limit the cubemap
- `src/app/canvas.rs::draw_image_workspace` — where the error overlay renders
  first, but does not itself catch wgpu async validation errors

## Test app

Kept at `/tmp/ddstest` per user instruction. Has a working DDS decode profile
(BC5U/DXT-family/BC7/uncompressed all decode fine via `image_dds`); the wgpu
texture-creation portion needs the 29-API fixes above before it will compile.