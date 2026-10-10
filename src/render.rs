//! The image canvas render pass: draws the displayed image through a custom
//! wgpu pipeline inside egui's frame (via `egui_wgpu::CallbackTrait`).
//!
//! Why a custom pass: sharp (pixel) sampling is a shader decision, not a
//! sampler switch, and images larger than the GPU's max texture dimension are
//! chunked into tiles that this pass reassembles in image space. Textures are
//! RGBA8 decoded on the CPU (native-format upload is a later milestone); the
//! shader passes sRGB-encoded values through untouched, matching egui's
//! convention.
//!
//! Resource model: the loader worker builds a [`RenderImage`] (tile textures,
//! bind groups, vertex buffers) using the layouts from [`ImageRendererState`]
//! (created at startup against the surface format and shared into the worker
//! so bind groups and pipeline agree). Per frame, the canvas enqueues a
//! [`DrawImageCallback`] holding only view state; `prepare` writes the
//! frame's uniform buffer, `paint` draws every tile of the displayed layer.

use std::sync::{Arc, Mutex};

use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("render/image.wgsl");

/// Vertex data: the tile's corner in image-normalized space and its texture
/// coordinate. Six vertices per tile (two triangles), emitted once at upload.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    norm: [f32; 2],
    uv: [f32; 2],
}

/// Per-frame uniforms. Layout must match `Globals` in image.wgsl (explicit
/// padding so there is no implicit padding for bytemuck to trip on).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport_px: [f32; 2],
    viewport_size_px: [f32; 2],
    screen_size_px: [f32; 2],
    _pad0: [f32; 2],
    tint: [f32; 4],
    sharp: f32,
    _pad1: [f32; 3],
}

/// Shared GPU state: pipeline and bind group layouts. Built once at startup
/// against the surface's format and inserted into egui_wgpu's
/// `callback_resources`; also handed to the loader worker so its bind groups
/// are created from the *same* layout objects the pipeline uses.
pub struct ImageRendererState {
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) tile_layout: wgpu::BindGroupLayout,
    pub(crate) globals_layout: wgpu::BindGroupLayout,
}

impl ImageRendererState {
    /// Create against the given target format (the surface's format).
    pub(crate) fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let tile_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photon_tile_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("photon_globals_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("photon_image_pipeline_layout"),
            bind_group_layouts: &[Some(&tile_layout), Some(&globals_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photon_image_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
            ],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("photon_image_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[vertex_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            tile_layout,
            globals_layout,
        }
    }
}

/// One chunk of a (possibly oversized) layer with its GPU resources. The
/// vertex buffer bakes the tile's normalized quad and its uv sub-rect.
pub(crate) struct Tile {
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) vertex_buffer: wgpu::Buffer,
}

/// One layer (face/array entry) of an image: its tiles in row-major order.
pub(crate) struct Layer {
    pub(crate) tiles: Vec<Tile>,
}

/// A fully GPU-resident image, ready to draw. Built by the loader worker.
pub struct RenderImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) is_cubemap: bool,
    pub(crate) layers: Vec<Layer>,
}

impl RenderImage {
    pub(crate) fn dims(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Tile-grid math: split a `width`×`height` image into chunks of at most
/// `max_dim` per side, returning `(x, y, w, h)` rects in row-major order.
/// Pure function; unit-tested.
pub(crate) fn chunk_grid(width: u32, height: u32, max_dim: u32) -> Vec<(u32, u32, u32, u32)> {
    let mut rects = Vec::new();
    let mut y = 0;
    while y < height {
        let mut x = 0;
        let ch = max_dim.min(height - y);
        while x < width {
            let cw = max_dim.min(width - x);
            rects.push((x, y, cw, ch));
            x += cw;
        }
        y += ch;
    }
    rects
}

/// Build the GPU-resident form of one decoded RGBA8 layer: chunked when the
/// layer exceeds `max_dim`, each chunk getting a texture, bind group (both
/// samplers; the shader picks) and its vertex quad. Texture uploads go
/// through `queue`.
pub(crate) fn build_layer(rgba: &[u8], width: u32, height: u32, ctx: &RenderContext<'_>) -> Layer {
    let device = ctx.device;
    let queue = ctx.queue;
    let max_dim = ctx.max_dim;
    let tile_layout = ctx.tile_layout;
    let linear = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("photon_linear_sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("photon_nearest_sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let bytes_per_row = width * 4;

    let tiles = chunk_grid(width, height, max_dim)
        .into_iter()
        .map(|(tx, ty, tw, th)| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("photon_tile_texture"),
                size: wgpu::Extent3d {
                    width: tw,
                    height: th,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            // The source buffer is the whole layer: slice this tile's rows
            // out via the copy layout instead of copying on the CPU. Each
            // copied row is the full layer width; only the tile's columns
            // are consumed.
            let row_offset = ty as usize * bytes_per_row as usize;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: Default::default(),
                },
                &rgba[row_offset..],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    rows_per_image: None,
                    bytes_per_row: Some(bytes_per_row),
                },
                wgpu::Extent3d {
                    width: tw,
                    height: th,
                    depth_or_array_layers: 1,
                },
            );

            let view = texture.create_view(&Default::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("photon_tile_bind_group"),
                layout: tile_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&linear),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&nearest),
                    },
                ],
            });

            let uv_left = tx as f32 / width as f32;
            let uv_top = ty as f32 / height as f32;
            let uv_right = (tx + tw) as f32 / width as f32;
            let uv_bottom = (ty + th) as f32 / height as f32;
            let vertices = [
                Vertex {
                    norm: [0.0, 0.0],
                    uv: [uv_left, uv_top],
                },
                Vertex {
                    norm: [1.0, 0.0],
                    uv: [uv_right, uv_top],
                },
                Vertex {
                    norm: [0.0, 1.0],
                    uv: [uv_left, uv_bottom],
                },
                Vertex {
                    norm: [1.0, 0.0],
                    uv: [uv_right, uv_top],
                },
                Vertex {
                    norm: [1.0, 1.0],
                    uv: [uv_right, uv_bottom],
                },
                Vertex {
                    norm: [0.0, 1.0],
                    uv: [uv_left, uv_bottom],
                },
            ];
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("photon_tile_vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

            Tile {
                bind_group,
                vertex_buffer,
            }
        })
        .collect();

    Layer { tiles }
}

/// The device-side context the loader worker needs to build a RenderImage.
pub(crate) struct RenderContext<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a wgpu::Queue,
    pub(crate) tile_layout: &'a wgpu::BindGroupLayout,
    pub(crate) max_dim: u32,
}

/// Build the GPU-resident form of a decoded image (all layers, chunked as
/// needed). Called by the loader worker.
pub(crate) fn build_render_image(
    ctx: &RenderContext<'_>,
    layers_rgba: Vec<Vec<u8>>,
    width: u32,
    height: u32,
    is_cubemap: bool,
) -> RenderImage {
    let layers = layers_rgba
        .into_iter()
        .map(|rgba| build_layer(&rgba, width, height, ctx))
        .collect();
    RenderImage {
        width,
        height,
        is_cubemap,
        layers,
    }
}

/// Per-frame callback: `prepare` writes the frame uniform, `paint` draws the
/// displayed layer's tiles.
pub(crate) struct DrawImageCallback {
    pub(crate) image: Arc<RenderImage>,
    pub(crate) layer: usize,
    pub(crate) sharp: bool,
    pub(crate) tint: [f32; 4],
    /// On-screen rect of the whole image, in egui points.
    pub(crate) rect_points: egui::Rect,
    uniform_slot: Mutex<Option<wgpu::BindGroup>>,
}

impl DrawImageCallback {
    pub(crate) fn new(
        image: Arc<RenderImage>,
        layer: usize,
        sharp: bool,
        tint: [f32; 4],
        rect_points: egui::Rect,
    ) -> Self {
        Self {
            image,
            layer,
            sharp,
            tint,
            rect_points,
            uniform_slot: Mutex::new(None),
        }
    }
}

impl egui_wgpu::CallbackTrait for DrawImageCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(state) = resources.get::<ImageRendererState>() else {
            return Vec::new();
        };
        let ppp = screen_descriptor.pixels_per_point;
        let globals = Globals {
            viewport_px: [
                self.rect_points.min.x * ppp,
                self.rect_points.min.y * ppp,
            ],
            viewport_size_px: [
                self.rect_points.width() * ppp,
                self.rect_points.height() * ppp,
            ],
            screen_size_px: [
                screen_descriptor.size_in_pixels[0] as f32,
                screen_descriptor.size_in_pixels[1] as f32,
            ],
            _pad0: [0.0; 2],
            tint: self.tint,
            sharp: self.sharp as u8 as f32,
            _pad1: [0.0; 3],
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("photon_frame_globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photon_frame_globals_bind_group"),
            layout: &state.globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        *self.uniform_slot.lock().expect("uniform slot poisoned") = Some(bind_group);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(state) = resources.get::<ImageRendererState>() else {
            return;
        };
        let Some(buffer) = self
            .uniform_slot
            .lock()
            .expect("uniform slot poisoned")
            .take()
        else {
            return;
        };
        let Some(layer) = self.image.layers.get(self.layer) else {
            return;
        };

        render_pass.set_pipeline(&state.pipeline);
        render_pass.set_bind_group(1, &buffer, &[]);
        for tile in &layer.tiles {
            render_pass.set_bind_group(0, &tile.bind_group, &[]);
            render_pass.set_vertex_buffer(0, tile.vertex_buffer.slice(..));
            render_pass.draw(0..6, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_grid_single_tile_when_within_limit() {
        assert_eq!(chunk_grid(100, 50, 8192), vec![(0, 0, 100, 50)]);
    }

    #[test]
    fn chunk_grid_splits_oversized_into_row_major_tiles() {
        // 3-wide limit over 5x5: two columns, two rows (3+2, 3+2).
        assert_eq!(
            chunk_grid(5, 5, 3),
            vec![(0, 0, 3, 3), (3, 0, 2, 3), (0, 3, 3, 2), (3, 3, 2, 2)]
        );
    }

    #[test]
    fn chunk_grid_handles_exact_multiple() {
        assert_eq!(chunk_grid(4, 2, 2), vec![(0, 0, 2, 2), (2, 0, 2, 2)]);
    }

    #[test]
    fn globals_layout_has_no_implicit_padding() {
        // WGSL struct: three vec2 (0,8,16), vec4 tint at 16-align -> 32,
        // sharp at 48, size rounded to 64.
        assert_eq!(std::mem::size_of::<Globals>(), 64);
        let g = Globals {
            viewport_px: [1.0, 2.0],
            viewport_size_px: [3.0, 4.0],
            screen_size_px: [5.0, 6.0],
            _pad0: [0.0; 2],
            tint: [1.0; 4],
            sharp: 1.0,
            _pad1: [0.0; 3],
        };
        let bytes: &[u8] = bytemuck::bytes_of(&g);
        assert_eq!(bytes.len(), 64);
    }
}
