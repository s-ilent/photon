# Photon

Photon is a fast, hardware-accelerated image viewer and browser written in Rust. It utilizes `egui` for the user interface and `wgpu` for rendering. 

---

## Keyboard Shortcuts

| Action | Shortcut (Win/Linux) | Shortcut (macOS) |
| :--- | :--- | :--- |
| **Open File/Dir** | `Ctrl + O` | `Cmd + O` |
| **Copy (or Crop-Copy)** | `Ctrl + C` | `Cmd + C` |
| **Paste Image** | `Ctrl + V` | `Cmd + V` |
| **Next Image** | `Space` / `Right Arrow` | `Space` / `Right Arrow` |
| **Previous Image** | `Backspace` / `Left Arrow` | `Backspace` / `Left Arrow` |
| **Cycle Mouse Mode** | `Tab` / `\` | `Tab` / `\` |

---

## Getting Started

### Running Photon
Run in release mode or image decoding will be slow:
```bash
cargo run --release -- /path/to/image/directory
```

### Building the Binary
```bash
cargo build --release
```
The executable will be located at `target/release/photon`.

---

## License

This project is licensed under either the [MIT License](LICENSE) or the [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0).

As of 0.1.0, almost all the code is written by AI. 

<sub>I don't think it did a very good job. But it works faster than the KDE native image viewer, which is all I needed.</sub>
