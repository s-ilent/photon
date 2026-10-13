# Vendored forks

## egui-winit 0.34.1 — `vendor/egui-winit`

**Why.** egui-winit intercepts Ctrl+V itself: it reads the clipboard as *text*
and returns unconditionally afterwards, before the key event is emitted. With
an image-only clipboard (this Wayland session; any platform whose text read
fails) it produces neither `Event::Paste` nor `Event::Key`, so an app that
pastes images never sees Ctrl+V — the toolbar button is the only working path.
Photon binds `Command::Paste` to Ctrl+V through the registry's raw-key fallback
(`src/input.rs`); that fallback can only fire if the key event arrives.

**The patch** lives in `vendor/egui-winit/src/lib.rs`, in the key-press handler,
marked `VENDOR PATCH (Photon)`: when the text read yields nothing, fall through
so `Event::Key` is emitted, instead of returning. A text clipboard behaves
exactly as upstream (a `Paste` event; the key stays swallowed). Find the hunk
with `grep -rn "VENDOR PATCH" vendor/`.

**Upgrading.**
1. `diff -ru vendor/egui-winit <registry-src>/egui-winit-<new>` to see upstream
   drift against the current tree.
2. Copy the new release over `vendor/egui-winit` (drop `.cargo-ok`,
   `.cargo_vcs_info.json`, `Cargo.lock`) and re-apply the marked hunk.
3. `env cargo check --release --all-targets`; confirm the path patch applies
   (`cargo tree -p photon | grep egui-winit` names the path, and cargo warns if
   the patch is unused). Update the version in this file.
