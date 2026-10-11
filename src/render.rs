//! Image canvas render pass (`egui_wgpu::CallbackTrait`): the loader worker
//! builds the GPU-resident [`RenderImage`] from [`ImageRendererState`] layouts;
//! each frame the canvas enqueues a [`DrawImageCallback`] to draw the tiles.

use std::sync::{Arc, Mutex};

use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("render/image.wgsl");

/// Image-normalized quad corner; also its texture coordinate.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    uv: [f32; 2],
}

/// Per-frame uniforms; layout must match `Globals` in image.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport_px: [f32; 2],
    viewport_size_px: [f32; 2],
    screen_size_px: [f32; 2],
    _pad0: [f32; 2],
    tint: [f32; 4],
    sharp: f32,
    /// 1.0 when the source bytes are linear-light, else 0.0.
    linear_source: f32,
    _pad1: [f32; 2],
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
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            }],
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

/// Key for `callback_resources` (TypeId-keyed): insert and get must name this
/// exact type, or the callback silently draws nothing.
pub(crate) type ImageRendererHandle = Arc<ImageRendererState>;

impl ImageRendererState {
    pub(crate) fn insert_into(
        resources: &mut egui_wgpu::CallbackResources,
        handle: ImageRendererHandle,
    ) {
        resources.insert(handle);
    }

    pub(crate) fn from_resources(
        resources: &egui_wgpu::CallbackResources,
    ) -> Option<&ImageRendererHandle> {
        resources.get::<ImageRendererHandle>()
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
    /// The transfer function of the uploaded bytes; the shader encodes a
    /// linear-light source to display space.
    pub color_space: crate::image_loader::ColorSpace,
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

/// Chunked GPU form of one RGBA8 layer. The sampler is linear; a nearest
/// sampler must not be added (sharp scaling lives in the shader).
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
                ],
            });

            let uv_left = tx as f32 / width as f32;
            let uv_top = ty as f32 / height as f32;
            let uv_right = (tx + tw) as f32 / width as f32;
            let uv_bottom = (ty + th) as f32 / height as f32;
            let vertices = [
                Vertex {
                    uv: [uv_left, uv_top],
                },
                Vertex {
                    uv: [uv_right, uv_top],
                },
                Vertex {
                    uv: [uv_left, uv_bottom],
                },
                Vertex {
                    uv: [uv_right, uv_top],
                },
                Vertex {
                    uv: [uv_right, uv_bottom],
                },
                Vertex {
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

/// The non-sRGB wgpu variant of a BC layout. The source colour space is handled
/// by the shader, so the sRGB texture formats are deliberately not used.
fn bc_texture_format(kind: crate::image_loader::BcKind) -> wgpu::TextureFormat {
    use crate::image_loader::BcKind;
    match kind {
        BcKind::Bc1 => wgpu::TextureFormat::Bc1RgbaUnorm,
        BcKind::Bc2 => wgpu::TextureFormat::Bc2RgbaUnorm,
        BcKind::Bc3 => wgpu::TextureFormat::Bc3RgbaUnorm,
        BcKind::Bc4Unorm => wgpu::TextureFormat::Bc4RUnorm,
        BcKind::Bc4Snorm => wgpu::TextureFormat::Bc4RSnorm,
        BcKind::Bc5Unorm => wgpu::TextureFormat::Bc5RgUnorm,
        BcKind::Bc5Snorm => wgpu::TextureFormat::Bc5RgSnorm,
        BcKind::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
    }
}

/// GPU form of one still-compressed BC layer: a single texture holding the whole
/// layer. A compressed stream cannot be sliced on a 4-texel block boundary, so
/// there is no chunking — the caller only keeps BC bytes within the texture
/// limit. The sampler stays linear.
fn build_bc_layer(
    bytes: &[u8],
    width: u32,
    height: u32,
    kind: crate::image_loader::BcKind,
    ctx: &RenderContext<'_>,
) -> Layer {
    let device = ctx.device;
    let queue = ctx.queue;
    let format = bc_texture_format(kind);
    let block_size = format
        .block_copy_size(None)
        .expect("BC texture format has a block size");
    let bytes_per_row = block_size * width.div_ceil(4);

    let linear = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("photon_linear_sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("photon_bc_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: Default::default(),
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            rows_per_image: None,
            bytes_per_row: Some(bytes_per_row),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("photon_bc_bind_group"),
        layout: ctx.tile_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&linear),
            },
        ],
    });

    // One tile: the full-layer uv quad.
    let vertices = [
        Vertex { uv: [0.0, 0.0] },
        Vertex { uv: [1.0, 0.0] },
        Vertex { uv: [0.0, 1.0] },
        Vertex { uv: [1.0, 0.0] },
        Vertex { uv: [1.0, 1.0] },
        Vertex { uv: [0.0, 1.0] },
    ];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("photon_bc_vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });

    Layer {
        tiles: vec![Tile {
            bind_group,
            vertex_buffer,
        }],
    }
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
    layers: Vec<Vec<u8>>,
    width: u32,
    height: u32,
    is_cubemap: bool,
    format: crate::image_loader::DecodedFormat,
    color_space: crate::image_loader::ColorSpace,
) -> RenderImage {
    let layers = layers
        .into_iter()
        .map(|bytes| match format {
            crate::image_loader::DecodedFormat::Rgba8 => build_layer(&bytes, width, height, ctx),
            crate::image_loader::DecodedFormat::Bc(kind) => {
                build_bc_layer(&bytes, width, height, kind, ctx)
            }
        })
        .collect();
    RenderImage {
        width,
        height,
        is_cubemap,
        color_space,
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
        let Some(state) = ImageRendererState::from_resources(resources) else {
            return Vec::new();
        };
        let state = state.as_ref();
        let ppp = screen_descriptor.pixels_per_point;
        let globals = Globals {
            viewport_px: [self.rect_points.min.x * ppp, self.rect_points.min.y * ppp],
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
            linear_source: matches!(
                self.image.color_space,
                crate::image_loader::ColorSpace::Linear
            ) as u8 as f32,
            _pad1: [0.0; 2],
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
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(state) = ImageRendererState::from_resources(resources) else {
            return;
        };
        let state = state.as_ref();
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

        // egui-wgpu sets the viewport to this callback's rect clamped to the
        // clip rect; override it to the full target so the image can overflow
        // the viewport when zoomed in (clipped by the scissor instead).
        render_pass.set_viewport(
            0.0,
            0.0,
            info.screen_size_px[0] as f32,
            info.screen_size_px[1] as f32,
            0.0,
            1.0,
        );
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
        assert_eq!(std::mem::size_of::<Globals>(), 64);
        assert_eq!(std::mem::offset_of!(Globals, sharp), 48);
        assert_eq!(std::mem::offset_of!(Globals, linear_source), 52);
        let g = Globals {
            viewport_px: [1.0, 2.0],
            viewport_size_px: [3.0, 4.0],
            screen_size_px: [5.0, 6.0],
            _pad0: [0.0; 2],
            tint: [1.0; 4],
            sharp: 1.0,
            linear_source: 1.0,
            _pad1: [0.0; 2],
        };
        assert_eq!(bytemuck::bytes_of(&g).len(), 64);
    }

    #[test]
    fn bc_texture_format_maps_to_non_srgb_variants() {
        use crate::image_loader::BcKind;
        assert_eq!(bc_texture_format(BcKind::Bc1), wgpu::TextureFormat::Bc1RgbaUnorm);
        assert_eq!(bc_texture_format(BcKind::Bc2), wgpu::TextureFormat::Bc2RgbaUnorm);
        assert_eq!(bc_texture_format(BcKind::Bc3), wgpu::TextureFormat::Bc3RgbaUnorm);
        assert_eq!(bc_texture_format(BcKind::Bc4Unorm), wgpu::TextureFormat::Bc4RUnorm);
        assert_eq!(bc_texture_format(BcKind::Bc4Snorm), wgpu::TextureFormat::Bc4RSnorm);
        assert_eq!(bc_texture_format(BcKind::Bc5Unorm), wgpu::TextureFormat::Bc5RgUnorm);
        assert_eq!(bc_texture_format(BcKind::Bc5Snorm), wgpu::TextureFormat::Bc5RgSnorm);
        assert_eq!(bc_texture_format(BcKind::Bc7), wgpu::TextureFormat::Bc7RgbaUnorm);
    }
}

/// Headless GPU tests; each skips when no adapter is available.
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use std::time::Duration;

    /// Returns `None` (so the test skips) when no adapter/device is available.
    fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let request = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter,
                compatible_surface: None,
            }))
        };
        let adapter = request(false).or_else(|_| request(true)).ok()?;

        // Request BC support only when the adapter has it, so the other GPU
        // tests still get a device and the BC test can skip cleanly.
        let mut required_features = wgpu::Features::empty();
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        {
            required_features |= wgpu::Features::TEXTURE_COMPRESSION_BC;
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("photon_gpu_test_device"),
            required_features,
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        }))
        .ok()?;
        Some((device, queue))
    }

    /// Render `image`'s layer 0 into a 4×4 `Rgba8Unorm` target and return the
    /// tightly packed RGBA8 pixels. The image's own colour space drives the
    /// shader's linear-source flag, matching `DrawImageCallback::prepare`.
    fn draw_offscreen(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        state: &ImageRendererState,
        image: &RenderImage,
    ) -> [u8; 64] {
        const SIZE: u32 = 4;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("photon_gpu_test_render_target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());

        let globals = Globals {
            viewport_px: [0.0, 0.0],
            viewport_size_px: [SIZE as f32, SIZE as f32],
            screen_size_px: [SIZE as f32, SIZE as f32],
            _pad0: [0.0; 2],
            tint: [1.0; 4],
            sharp: 0.0,
            linear_source: matches!(
                image.color_space,
                crate::image_loader::ColorSpace::Linear
            ) as u8 as f32,
            _pad1: [0.0; 2],
        };
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("photon_gpu_test_globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photon_gpu_test_globals_bg"),
            layout: &state.globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        // copy_texture_to_buffer requires a 256-aligned row pitch; pad, then
        // index with the aligned stride when reading pixels back.
        const ALIGNED_BPR: u32 = 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("photon_gpu_test_render_readback"),
            size: (ALIGNED_BPR * SIZE) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("photon_gpu_test_render_encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("photon_gpu_test_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&state.pipeline);
            pass.set_bind_group(1, &globals_bind, &[]);
            for tile in &image.layers[0].tiles {
                pass.set_bind_group(0, &tile.bind_group, &[]);
                pass.set_vertex_buffer(0, tile.vertex_buffer.slice(..));
                pass.draw(0..6, 0..1);
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: Default::default(),
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ALIGNED_BPR),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(20)),
            })
            .expect("device poll failed");
        rx.recv_timeout(Duration::from_secs(20))
            .expect("buffer mapping timed out")
            .expect("buffer mapping failed");

        let pixels = {
            let view = readback.slice(..).get_mapped_range();
            view.to_vec()
        };
        readback.unmap();

        // Strip the aligned row pitch so callers index the 4×4 image directly.
        let mut out = [0u8; 64];
        for y in 0..SIZE {
            for x in 0..SIZE {
                let i = (y * ALIGNED_BPR + x * 4) as usize;
                let o = ((y * SIZE + x) * 4) as usize;
                out[o..o + 4].copy_from_slice(&pixels[i..i + 4]);
            }
        }
        out
    }

    /// `image.wgsl` compiles and the pipeline builds on a live device.
    #[test]
    fn render_pipeline_builds_on_gpu() {
        let Some((device, _queue)) = headless_device() else {
            eprintln!("no GPU adapter; skipping render_pipeline_builds_on_gpu");
            return;
        };

        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _state = ImageRendererState::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let _ = device.poll(wgpu::PollType::Poll);
        let error = pollster::block_on(scope.pop());
        assert!(
            error.is_none(),
            "image.wgsl pipeline failed to build: {error:?}"
        );
    }

    /// Compute dispatch + read-back round-trip.
    #[test]
    fn trivial_compute_pass_runs() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter; skipping trivial_compute_pass_runs");
            return;
        };

        const COMPUTE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(1)
fn main() {
    data[0] = 42u;
    data[1] = 7u;
    data[2] = 123456u;
    data[3] = 0u;
}
"#;

        let storage = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("photon_gpu_test_storage"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("photon_gpu_test_readback"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("photon_gpu_test_compute_shader"),
            source: wgpu::ShaderSource::Wgsl(COMPUTE_WGSL.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("photon_gpu_test_compute_pipeline"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photon_gpu_test_compute_bind_group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: storage.as_entire_binding(),
            }],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("photon_gpu_test_compute_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("photon_gpu_test_compute_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, 16);
        queue.submit(Some(encoder.finish()));

        // Bound the GPU wait: poll with a timeout *and* a matching channel
        // timeout, so a stuck submission fails the test rather than hanging
        // the suite.
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(20)),
            })
            .expect("device poll failed");
        rx.recv_timeout(Duration::from_secs(20))
            .expect("buffer mapping timed out")
            .expect("buffer mapping failed");

        let values = {
            let view = readback.slice(..).get_mapped_range();
            bytemuck::pod_read_unaligned::<[u32; 4]>(&view[..16])
        };
        readback.unmap();

        assert_eq!(values, [42u32, 7, 123_456, 0]);
    }

    /// Draws a known image offscreen and reads the pixels back.
    #[test]
    fn render_pass_draws_pixels_offscreen() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter; skipping render_pass_draws_pixels_offscreen");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let state = ImageRendererState::new(&device, format);

        // 2x2 source: red, green / blue, white.
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let ctx = RenderContext {
            device: &device,
            queue: &queue,
            tile_layout: &state.tile_layout,
            max_dim: 64,
        };
        let image = build_render_image(
            &ctx,
            vec![rgba],
            2,
            2,
            false,
            crate::image_loader::DecodedFormat::Rgba8,
            crate::image_loader::ColorSpace::Srgb,
        );

        let pixels = draw_offscreen(&device, &queue, &state, &image);
        let px = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * 4 + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        // Top-left quadrant samples source texel (0,0) = red; bottom-right (1,1)
        // = white. If the pass drew nothing, everything stays the black clear.
        assert!(
            px(0, 0)[0] > 200 && px(0, 0)[1] < 60,
            "top-left should be red, got {:?}",
            px(0, 0)
        );
        assert!(
            px(3, 3)[0] > 200 && px(3, 3)[1] > 200 && px(3, 3)[2] > 200,
            "bottom-right should be white, got {:?}",
            px(3, 3)
        );
    }

    /// A BC1 layer uploads to a compressed texture and decodes to red.
    #[test]
    fn bc1_texture_uploads_and_samples() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter; skipping bc1_texture_uploads_and_samples");
            return;
        };
        if !device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        {
            eprintln!("no BC texture support; skipping bc1_texture_uploads_and_samples");
            return;
        }
        use crate::image_loader::{BcKind, ColorSpace, DecodedFormat};

        let format = wgpu::TextureFormat::Rgba8Unorm;
        let state = ImageRendererState::new(&device, format);

        // One 4x4 BC1 block: color0 = 0xF800 (red), color1 = 0x0000, all index
        // bits 0, so every texel decodes to pure red.
        let block: Vec<u8> = vec![0x00, 0xF8, 0x00, 0x00, 0, 0, 0, 0];
        let ctx = RenderContext {
            device: &device,
            queue: &queue,
            tile_layout: &state.tile_layout,
            max_dim: 64,
        };
        let image = build_render_image(
            &ctx,
            vec![block],
            4,
            4,
            false,
            DecodedFormat::Bc(BcKind::Bc1),
            ColorSpace::Linear,
        );

        let pixels = draw_offscreen(&device, &queue, &state, &image);
        let px = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * 4 + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
        };
        let c = px(1, 1);
        assert!(
            c[0] > 200 && c[1] < 60 && c[2] < 60,
            "BC1 texel should be red, got {c:?}"
        );
    }
}
