//! The wgpu ink-renderer backend (wasm-only). It draws a
//! [`Scene`] — nothing more: all "what to draw" logic lives
//! in the pure, native-tested `scene` layer, so this file is just WebGPU plumbing
//! (a single alpha-blended, **textured** quad pipeline) plus the JS entry point. WebGPU
//! where available, WebGL2 fallback via wgpu's `webgl` backend and downlevel limits.
//!
//! **Text is real anti-aliased serif type**, not a bitmap: a glyph **atlas**
//! ([`atlas`](crate::atlas)) rasterized from the bundled EB Garamond (OFL) is uploaded as an
//! R8 coverage texture, and every label draws as atlas-UV quads through the same pipeline.
//! Shapes (washes, spirits, cards, pips) draw through that pipeline too — they sample the
//! atlas's solid-white texel, so a fill and a glyph are the same textured quad (one draw).
//! The board, the chrome, and the type all composite in one pass.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::scene::{Quad, Scene};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    color: [f32; 4],
    /// Atlas UV — a shape quad points at the solid-white texel (so `tex == 1`, a flat fill);
    /// a glyph quad points at its coverage cell (so `tex` is the anti-aliased alpha).
    uv: [f32; 2],
    /// Local coordinate within the quad, in **px from the quad centre** (so the fragment
    /// shader can evaluate a signed-distance rounded box for crisp corners + soft shadows).
    local: [f32; 2],
    /// SDF params: `[half_w, half_h, radius, softness]` in px. `radius < 0` ⇒ a plain textured
    /// quad (text glyphs + the board pass keep the old coverage-only path). `radius >= 0` ⇒ a
    /// rounded box: `softness == 0` is a crisp anti-aliased rounded fill (cards/buttons/panels);
    /// `softness > 0` is a **soft drop shadow** (a single quad whose alpha falls off over the
    /// softness band — one consistent light, no stacked grey halos).
    params: [f32; 4],
}

// One textured, alpha-blended quad pipeline draws EVERYTHING. Two modes, chosen per-quad by the
// SDF `radius` sign:
//   • radius < 0 — a PLAIN textured quad: shapes sample the atlas's solid-white texel (coverage
//     1 ⇒ a flat fill), glyphs sample their coverage cell (the .r channel is the anti-aliased
//     alpha). The board pass + all real type take this path.
//   • radius >= 0 — a ROUNDED-BOX SDF quad: the fragment computes the distance to a rounded
//     rectangle and either (softness 0) crisply anti-aliases the rounded edge for a card/button/
//     panel, or (softness > 0) renders a SOFT SHADOW whose alpha falls off smoothly over the
//     softness band. One light direction, soft blur, low opacity — a crafted drop, not a halo.
// `color * vec4(1,1,1, alpha)` gives crisp serif type, crisp rounded chrome, and soft shadows
// from the same shader + the same draw call.
const SHADER: &str = r#"
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) local: vec2<f32>,
    @location(3) params: vec4<f32>,
};
@group(0) @binding(0) var atlas_tex: texture_2d<f32>;
@group(0) @binding(1) var atlas_samp: sampler;
@vertex
fn vs(
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) local: vec2<f32>,
    @location(4) params: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(pos, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    out.local = local;
    out.params = params;
    return out;
}
// Signed distance from point `p` to a rounded box of half-extents `b` and corner radius `r`.
fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let radius = in.params.z;
    if (radius < 0.0) {
        // Plain textured quad (text + board): coverage-only, exactly as before.
        let coverage = textureSample(atlas_tex, atlas_samp, in.uv).r;
        return vec4<f32>(in.color.rgb, in.color.a * coverage);
    }
    let half = in.params.xy;
    let softness = in.params.w;
    let d = sd_round_box(in.local, half, radius);
    if (softness > 0.0) {
        // A soft drop shadow: full alpha just inside the box, falling to 0 over the softness
        // band outside it (a smooth, low-opacity penumbra — one quad, no stacking).
        let a = 1.0 - smoothstep(-softness * 0.35, softness, d);
        return vec4<f32>(in.color.rgb, in.color.a * a);
    }
    // A crisp rounded fill: anti-alias the edge over ~1px (fwidth tracks the screen-space
    // derivative, so corners stay sharp at any DPR).
    let aa = max(fwidth(d), 0.75);
    let a = 1.0 - smoothstep(-aa, aa, d);
    return vec4<f32>(in.color.rgb, in.color.a * a);
}
"#;

/// The board renderer, owned by JS for the life of the page.
#[wasm_bindgen]
pub struct WebRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    capacity: u64,
    /// The glyph-atlas bind group (the coverage texture + sampler) the textured pipeline samples.
    atlas_bind: wgpu::BindGroup,
    /// The built glyph atlas (per-glyph layout + the solid-texel UV). Real serif type, no DOM.
    atlas: crate::atlas::GlyphAtlas,
    /// Short card labels, indexed by card id (the catalog is id-ordered).
    names: Vec<String>,
}

const MAX_QUADS: u64 = 8192;
const PAPER: wgpu::Color = wgpu::Color {
    r: 0.96,
    g: 0.94,
    b: 0.89,
    a: 1.0,
};

#[wasm_bindgen]
impl WebRenderer {
    /// Async because adapter/device acquisition is async on the web. Returns a
    /// renderer bound to `canvas`; call a `draw_*` method per frame.
    pub async fn new(canvas: HtmlCanvasElement) -> Result<WebRenderer, JsValue> {
        console_error_panic_hook::set_once();
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(err)?;
        // Downlevel WebGL2 limits keep the same path working without WebGPU.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("recollect-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(err)?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ink-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        // ── Build the glyph atlas (real serif type) + upload it as an R8 coverage texture. ──
        let atlas = crate::atlas::GlyphAtlas::build();
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.width),
                rows_per_image: Some(atlas.height),
            },
            wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
        );
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas-bgl"),
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
        let atlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas-bind"),
            layout: &atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ink-layout"),
            bind_group_layouts: &[Some(&atlas_bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ink-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let capacity = MAX_QUADS * 6 * std::mem::size_of::<Vertex>() as u64;
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ink-vertices"),
            size: capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let names = recollect_core::cards::canon_catalog()
            .iter()
            .map(|c| crate::scene::short_board_name(&c.name))
            .collect();

        Ok(WebRenderer {
            surface,
            device,
            queue,
            config,
            pipeline,
            vertices,
            capacity,
            atlas_bind,
            atlas,
            names,
        })
    }

    /// Resize the surface (call on canvas resize).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Draw a scene from its JSON (the `serde` shape of [`Scene`] — the JS side
    /// gets it from the rules core's `PlayerView`/`TeamView`). Convenience wrapper
    /// around [`draw_scene`](Self::draw_scene) so JS need not know the Rust type.
    pub fn draw_view(&mut self, player_view_json: &str) -> Result<(), JsValue> {
        let view: recollect_core::view::PlayerView =
            serde_json::from_str(player_view_json).map_err(err)?;
        let scene = crate::scene::build_player_scene(&view, &self.names);
        self.draw_scene(&scene);
        Ok(())
    }

    /// As [`draw_view`](Self::draw_view), plus the movement cues (a
    /// [`MoveCues`](crate::scene::MoveCues) JSON `{ "movable": [..], "sick": [..] }`):
    /// a green corner dot on a Mobile spirit that can still step, a dim one on a
    /// rested / summoning-sick one. The local shell computes the cues from its
    /// engine; an online client passes `{}` (no engine) until the view carries them.
    pub fn draw_view_cued(
        &mut self,
        player_view_json: &str,
        cues_json: &str,
    ) -> Result<(), JsValue> {
        let view: recollect_core::view::PlayerView =
            serde_json::from_str(player_view_json).map_err(err)?;
        let cues: crate::scene::MoveCues = serde_json::from_str(cues_json).unwrap_or_default();
        let scene = crate::scene::build_player_scene_cued(&view, &self.names, &cues);
        self.draw_scene(&scene);
        Ok(())
    }

    /// As [`draw_view_cued`](Self::draw_view_cued), plus the input overlays
    /// (an [`Interaction`](crate::scene::Interaction) JSON `{ "legal": [..],
    /// "selected": t?, "focus": t? }`): a green glow on each legal target, the
    /// seat-ink ring on the picked-up spirit, and a gold focus ring on the keyboard
    /// cursor's tile. The JS shell derives the overlay from the engine's legal moves
    /// (pointer/keyboard selection); an empty interaction draws the plain board.
    pub fn draw_view_interactive(
        &mut self,
        player_view_json: &str,
        cues_json: &str,
        interaction_json: &str,
    ) -> Result<(), JsValue> {
        let view: recollect_core::view::PlayerView =
            serde_json::from_str(player_view_json).map_err(err)?;
        let cues: crate::scene::MoveCues = serde_json::from_str(cues_json).unwrap_or_default();
        let inter: crate::scene::Interaction =
            serde_json::from_str(interaction_json).unwrap_or_default();
        let scene = crate::scene::build_player_scene_interactive(&view, &self.names, &cues, &inter);
        self.draw_scene(&scene);
        Ok(())
    }

    /// Draw a 2v2 `TeamView` (a 6×6 board) — same pipeline, wider grid.
    pub fn draw_team(&mut self, team_view_json: &str) -> Result<(), JsValue> {
        let view: recollect_core::view::TeamView =
            serde_json::from_str(team_view_json).map_err(err)?;
        let scene = crate::scene::build_team_scene(&view, &self.names);
        self.draw_scene(&scene);
        Ok(())
    }

    /// As [`draw_team`](Self::draw_team), plus the movement cues for the live
    /// 2v2 frame (same [`MoveCues`](crate::scene::MoveCues) JSON shape).
    pub fn draw_team_cued(&mut self, team_view_json: &str, cues_json: &str) -> Result<(), JsValue> {
        let view: recollect_core::view::TeamView =
            serde_json::from_str(team_view_json).map_err(err)?;
        let cues: crate::scene::MoveCues = serde_json::from_str(cues_json).unwrap_or_default();
        let scene = crate::scene::build_team_scene_cued(&view, &self.names, &cues);
        self.draw_scene(&scene);
        Ok(())
    }

    /// As [`draw_team_cued`](Self::draw_team_cued), plus the input overlays for
    /// the live 2v2 frame (same [`Interaction`](crate::scene::Interaction) JSON shape
    /// as [`draw_view_interactive`](Self::draw_view_interactive)).
    pub fn draw_team_interactive(
        &mut self,
        team_view_json: &str,
        cues_json: &str,
        interaction_json: &str,
    ) -> Result<(), JsValue> {
        let view: recollect_core::view::TeamView =
            serde_json::from_str(team_view_json).map_err(err)?;
        let cues: crate::scene::MoveCues = serde_json::from_str(cues_json).unwrap_or_default();
        let inter: crate::scene::Interaction =
            serde_json::from_str(interaction_json).unwrap_or_default();
        let scene = crate::scene::build_team_scene_interactive(&view, &self.names, &cues, &inter);
        self.draw_scene(&scene);
        Ok(())
    }

    /// Animate between two views at progress `t` (0→1) for the rAF loop: build both
    /// scenes and draw their [`Scene::interpolate`](crate::scene::Scene::interpolate)
    /// (play fades in, banish fades out). `team` selects the 6×6 TeamView build;
    /// `t >= 1` draws `next` exactly.
    pub fn draw_blend(
        &mut self,
        prev_json: &str,
        next_json: &str,
        t: f32,
        team: bool,
    ) -> Result<(), JsValue> {
        let scene = if team {
            let next: recollect_core::view::TeamView =
                serde_json::from_str(next_json).map_err(err)?;
            let next_scene = crate::scene::build_team_scene(&next, &self.names);
            if t >= 1.0 {
                next_scene
            } else {
                let prev: recollect_core::view::TeamView =
                    serde_json::from_str(prev_json).map_err(err)?;
                let prev_scene = crate::scene::build_team_scene(&prev, &self.names);
                crate::scene::Scene::interpolate(&prev_scene, &next_scene, t)
            }
        } else {
            let next: recollect_core::view::PlayerView =
                serde_json::from_str(next_json).map_err(err)?;
            let next_scene = crate::scene::build_player_scene(&next, &self.names);
            if t >= 1.0 {
                next_scene
            } else {
                let prev: recollect_core::view::PlayerView =
                    serde_json::from_str(prev_json).map_err(err)?;
                let prev_scene = crate::scene::build_player_scene(&prev, &self.names);
                crate::scene::Scene::interpolate(&prev_scene, &next_scene, t)
            }
        };
        self.draw_scene(&scene);
        Ok(())
    }

    /// A screen-reader text description of the board (round, whose turn, every
    /// occupied tile) for a visually-hidden region beside the canvas, so the wgpu
    /// canvas isn't opaque to assistive tech. `team` selects the 6×6 TeamView.
    pub fn board_aria(&self, view_json: &str, team: bool) -> Result<String, JsValue> {
        if team {
            let v: recollect_core::view::TeamView = serde_json::from_str(view_json).map_err(err)?;
            Ok(crate::scene::board_description_team(&v, &self.names))
        } else {
            let v: recollect_core::view::PlayerView =
                serde_json::from_str(view_json).map_err(err)?;
            Ok(crate::scene::board_description(&v, &self.names))
        }
    }

    /// Draw the **whole in-canvas game shell** for a full-bleed
    /// `vw`×`vh` (CSS-px) viewport: the board (the hero, placed into its central
    /// rectangle) plus the HUD, the opponent strip, the hand tray, and the floating
    /// buttons. `model_json` is a [`ShellModel`](crate::shell::ShellModel) (built by
    /// [`LocalGame::shell_model_json`](crate::LocalGame::shell_model_json)); the
    /// pure [`build_shell`](crate::shell::build_shell) lays it out, and this only
    /// maps the resulting screen-space primitives to clip space. The board's own
    /// scene is composited in by mapping its tile-grid quads/labels through the
    /// board rectangle, so the existing board look is unchanged — just placed.
    pub fn draw_shell(&mut self, model_json: &str, vw: f32, vh: f32) -> Result<(), JsValue> {
        let model: crate::shell::ShellModel = serde_json::from_str(model_json).map_err(err)?;
        let scene = crate::shell::build_shell(&model, vw, vh);
        self.draw_shell_scene(&scene);
        Ok(())
    }

    /// The board's pixel rectangle within a `vw`×`vh` shell viewport
    /// (`{ "x", "y", "w", "h" }`, in canvas backing px). The shell draws the board as
    /// a centered sub-rectangle, so the JS click/keyboard mapping needs this to turn
    /// a canvas hit into a board tile (the board interaction keeps
    /// working under the shell — it maps into this rect instead of the whole canvas).
    pub fn shell_board_rect_json(&self, vw: f32, vh: f32) -> String {
        let r = crate::shell::board_rect(vw, vh);
        serde_json::json!({ "x": r.x, "y": r.y, "w": r.w, "h": r.h }).to_string()
    }

    /// The **result-screen** action button hit-test rects for a `vw`×`vh`
    /// viewport, as JSON (`[{ "verb", "x", "y", "w", "h" } …]` — the Rematch / New
    /// opponent / Back to site buttons), so the JS bridge maps a canvas tap onto the right
    /// verb (one source with the in-canvas draw). `result_json` is a
    /// [`ResultScreen`](crate::shell::ResultScreen); a malformed / null one yields `[]`.
    pub fn result_action_rects_json(&self, result_json: &str, vw: f32, vh: f32) -> String {
        let res: crate::shell::ResultScreen = match serde_json::from_str(result_json) {
            Ok(r) => r,
            Err(_) => return "[]".into(),
        };
        let rects: Vec<serde_json::Value> = crate::shell::result_action_rects(&res, vw, vh)
            .into_iter()
            .map(|(verb, r)| serde_json::json!({ "verb": verb, "x": r.x, "y": r.y, "w": r.w, "h": r.h }))
            .collect();
        serde_json::to_string(&rects).unwrap()
    }
}

impl WebRenderer {
    /// The actual draw: upload the scene's quads (sorted by layer) as triangles,
    /// clear to paper, and render in one pass.
    pub fn draw_scene(&mut self, scene: &Scene) {
        let mut quads: Vec<Quad> = scene.quads.clone();
        quads.sort_by_key(|q| q.layer);

        let bw = scene.board_w.max(1) as f32;
        let bh = scene.board_h.max(1) as f32;
        // Tile-grid (x right, y down) → clip space (x right, y up).
        let to_clip = |x: f32, y: f32| [x / bw * 2.0 - 1.0, 1.0 - y / bh * 2.0];
        let solid = self.atlas.solid_uv;

        let mut verts: Vec<Vertex> = Vec::with_capacity(quads.len() * 6 + scene.labels.len() * 24);
        // Helper: push one quad (in tile-grid coords) with a UV rect.
        let push_quad = |verts: &mut Vec<Vertex>,
                         x: f32,
                         y: f32,
                         w: f32,
                         h: f32,
                         c: [f32; 4],
                         uv0: (f32, f32),
                         uv1: (f32, f32)| {
            let tl = (to_clip(x, y), [uv0.0, uv0.1]);
            let tr = (to_clip(x + w, y), [uv1.0, uv0.1]);
            let bl = (to_clip(x, y + h), [uv0.0, uv1.1]);
            let br = (to_clip(x + w, y + h), [uv1.0, uv1.1]);
            // The standalone board pass is all plain textured quads (board fills + glyphs):
            // `params.z < 0` selects the coverage-only path, so `local` is unused (zeroed).
            for (pos, uv) in [tl, bl, br, tl, br, tr] {
                verts.push(Vertex {
                    pos,
                    color: c,
                    uv,
                    local: [0.0, 0.0],
                    params: [0.0, 0.0, -1.0, 0.0],
                });
            }
        };
        // The board's own quads (washes / grid / spirits / pips) — flat fills (solid texel).
        for q in &quads {
            let c = [q.color.r, q.color.g, q.color.b, q.color.a];
            push_quad(&mut verts, q.x, q.y, q.w, q.h, c, solid, solid);
        }
        // The tile labels (spirit short-names + HP / stats) — real atlas serif glyphs, in
        // tile-grid coords at each label's own `size` (the atlas returns quads in the same space
        // as its input; the compact board card uses a smaller size for its stat foot).
        for label in &scene.labels {
            let c = [label.color.r, label.color.g, label.color.b, label.color.a];
            for g in self
                .atlas
                .layout_centered(label.x, label.y, label.size, &label.text)
            {
                push_quad(
                    &mut verts,
                    g.x,
                    g.y,
                    g.w,
                    g.h,
                    c,
                    (g.u0, g.v0),
                    (g.u1, g.v1),
                );
            }
        }
        let bytes = bytemuck::cast_slice(&verts);
        if bytes.len() as u64 > self.capacity {
            return; // over budget for this frame; the scene cap is generous
        }
        self.queue.write_buffer(&self.vertices, 0, bytes);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ink-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(PAPER),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.atlas_bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..verts.len() as u32, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    /// Draw a composed [`ShellScene`](crate::shell::ShellScene):
    /// the board scene mapped into its rectangle, then the chrome rects + text,
    /// all flattened to **viewport-pixel** quads and rendered in one pass. The
    /// viewport-px → clip transform is shared by board and chrome, so the two read
    /// as one surface (no second coordinate seam on screen).
    pub fn draw_shell_scene(&mut self, scene: &crate::shell::ShellScene) {
        let (vw, vh) = (scene.vw.max(1.0), scene.vh.max(1.0));
        let mut px: Vec<PxQuad> = Vec::new();

        // The composite bands: the page ground + panel frames sit BELOW the board; the
        // card/FAB BODIES sit above it; the Detail marks (cost discs, pips, rings, art
        // plates, stat pills) sit above the bodies; text is topmost. Each ShellLayer gets
        // its OWN order so a card-body GRADIENT (Card) always draws BELOW its Detail marks
        // even though grads + flats are uploaded in separate passes — otherwise the body
        // gradient would paint over the cost disc / art plate. (The board sits between the
        // panels and the cards; the regions don't overlap so the panel↔board order is moot.)
        const BOARD_ORDER: u8 = 2;
        const TEXT_ORDER: u8 = 6;
        let order_of = |layer: crate::shell::ShellLayer| {
            use crate::shell::ShellLayer::*;
            match layer {
                Ground => 0,
                Panel => 1,
                Card => 3,
                Detail => 4,
                Text => TEXT_ORDER,
            }
        };

        // Every SHAPE quad samples the atlas's solid-white texel (coverage 1 ⇒ a flat fill).
        let solid = self.atlas.solid_uv;

        // 0) The soft drop shadows (item 1) — each composited JUST UNDER its caster's layer
        // band (so a card's shadow lands on the panel below it, the card body over the shadow).
        // The SDF shader grows the box by `softness` to render the penumbra, so we expand the
        // quad's geometry here to give that falloff room (otherwise it'd be clipped to the box).
        for sh in &scene.shadows {
            let grow = sh.softness * 1.6;
            // The shadow band sits at the caster's order minus a hair, so it always draws first.
            let order = order_of(sh.layer).saturating_sub(1);
            // The geometry grows to give the penumbra canvas, but the SDF box stays the CASTER's
            // half-extents (`sdf_half`), so the soft falloff radiates from the caster's edge.
            px.push(PxQuad {
                x: sh.x - grow,
                y: sh.y - grow,
                w: sh.w + 2.0 * grow,
                h: sh.h + 2.0 * grow,
                color: sh.color,
                color2: sh.color,
                uv0: solid,
                uv1: solid,
                order,
                radius: sh.radius.max(0.0),
                softness: sh.softness,
                sdf_half: Some((sh.w * 0.5, sh.h * 0.5)),
            });
        }

        // 1) The chrome ground/panels/cards/details, in viewport px already (flat fills). The
        // `radius` is the rounded-box corner the SDF shader draws — so a card/button/panel reads
        // as a real rounded chip with anti-aliased corners. A
        // full-bleed ground/band (radius 0) stays square; a 1px rule or hairline (radius 0)
        // stays crisp.
        for r in &scene.rects {
            px.push(PxQuad {
                x: r.x,
                y: r.y,
                w: r.w,
                h: r.h,
                color: r.color,
                color2: r.color,
                uv0: solid,
                uv1: solid,
                order: order_of(r.layer),
                radius: r.radius,
                softness: 0.0,
                sdf_half: None,
            });
        }
        // 1b) The chrome GRADIENTS (the hero surfaces — ground, bands, cards, FABs, panels):
        // top→bottom vertical gradients, composited in their layer band just like the flats —
        // and rounded by the same SDF `radius`.
        for g in &scene.grads {
            px.push(PxQuad {
                x: g.x,
                y: g.y,
                w: g.w,
                h: g.h,
                color: g.top,
                color2: g.bottom,
                uv0: solid,
                uv1: solid,
                order: order_of(g.layer),
                radius: g.radius,
                softness: 0.0,
                sdf_half: None,
            });
        }

        // 2) The board scene, mapped tile-grid → the board rectangle (px). It draws
        // ABOVE the chrome ground/panel (so the page frame shows around it) but its
        // own internal layers keep their relative order via the Scene's sort.
        let placement =
            crate::shell::place_board(&scene.board_rect, scene.board.board_w, scene.board.board_h);
        // The board's own quads (washes / spirits / pips), mapped into px (flat shapes).
        let mut board_quads: Vec<Quad> = scene.board.quads.clone();
        board_quads.sort_by_key(|q| q.layer);
        for q in &board_quads {
            let (x0, y0) = placement.map(q.x, q.y);
            let (x1, y1) = placement.map(q.x + q.w, q.y + q.h);
            px.push(PxQuad::plain(
                x0,
                y0,
                x1 - x0,
                y1 - y0,
                q.color,
                q.color,
                solid,
                solid,
                BOARD_ORDER,
            ));
        }
        // The board's tile LABELS (spirit short-names + HP) — real atlas glyphs, mapped
        // through the placement, drawn just over the board band (so they sit on the spirit).
        // The label height is in tile-grid units; convert to px via the placement's y-scale.
        for label in &scene.board.labels {
            let glyph_px = label.size * placement.sy;
            let (lx, ly) = placement.map(label.x, label.y);
            for g in self.atlas.layout_centered(lx, ly, glyph_px, &label.text) {
                px.push(PxQuad::plain(
                    g.x,
                    g.y,
                    g.w,
                    g.h,
                    label.color,
                    label.color,
                    (g.u0, g.v0),
                    (g.u1, g.v1),
                    BOARD_ORDER + 1, // just over the board fills
                ));
            }
        }

        // 3) The chrome TEXT — real anti-aliased serif glyphs from the atlas, topmost. The
        // atlas lays each label out centred on (cx, cy); shift cx for the requested alignment.
        for t in &scene.texts {
            let w = self.atlas.text_width(&t.text, t.size);
            let cx = match t.align {
                crate::shell::Align::Left => t.x + w / 2.0,
                crate::shell::Align::Center => t.x,
                crate::shell::Align::Right => t.x - w / 2.0,
            };
            let glyphs = self.atlas.layout_centered(cx, t.y, t.size, &t.text);
            // Item 7 — faux-bold: the atlas is one weight, so a bold label is dilated by drawing
            // each glyph a few times with a tiny offset (≈ a stroke-thickening), giving the
            // character names real presence without a second font. A regular label draws once.
            let offsets: &[(f32, f32)] = if t.bold {
                let d = (t.size * 0.022).clamp(0.4, 1.6);
                &[(0.0, 0.0), (d, 0.0), (0.0, d), (d, d), (-d * 0.5, 0.0)]
            } else {
                &[(0.0, 0.0)]
            };
            px.reserve(glyphs.len() * offsets.len());
            for g in &glyphs {
                for (ox, oy) in offsets {
                    px.push(PxQuad::plain(
                        g.x + ox,
                        g.y + oy,
                        g.w,
                        g.h,
                        t.color,
                        t.color,
                        (g.u0, g.v0),
                        (g.u1, g.v1),
                        TEXT_ORDER,
                    ));
                }
            }
        }

        // Stable sort by order band so panels < board < text composite correctly
        // (within a band, insertion order — already correct per pass).
        px.sort_by_key(|q| q.order);
        self.present_px_quads(&px, vw, vh);
    }

    /// Upload `quads` (viewport px; x right / y down) and render one pass, clearing
    /// to paper. The viewport-px → clip transform is `x/vw*2-1`, `1-y/vh*2`.
    fn present_px_quads(&mut self, quads: &[PxQuad], vw: f32, vh: f32) {
        let to_clip = |x: f32, y: f32| [x / vw * 2.0 - 1.0, 1.0 - y / vh * 2.0];
        let mut verts: Vec<Vertex> = Vec::with_capacity(quads.len() * 6);
        for q in quads {
            // Top colour for the top two verts, bottom colour for the bottom two — a flat
            // quad has color == color2; a gradient quad interpolates between them. The UV rect
            // maps each corner to the atlas (solid texel for a shape, glyph cell for text).
            let ct = [q.color.r, q.color.g, q.color.b, q.color.a];
            let cb = [q.color2.r, q.color2.g, q.color2.b, q.color2.a];
            let (u0, v0) = q.uv0;
            let (u1, v1) = q.uv1;
            // `local` spans the GEOMETRY (corners map to ±geometry-half), so a grown shadow quad
            // still gives the penumbra canvas. The SDF box half-extents come from `sdf_half` when
            // set (a shadow: the soft falloff radiates from the CASTER's edge, inset within the
            // grown geometry), else the geometry itself (a normal rounded fill fills its quad).
            let (ghw, ghh) = (q.w * 0.5, q.h * 0.5);
            let (shw, shh) = q.sdf_half.unwrap_or((ghw, ghh));
            let r = if q.radius < 0.0 {
                -1.0
            } else {
                q.radius.min(shw.min(shh))
            };
            let params = [shw, shh, r, q.softness];
            // local = px offset from centre at each corner (the SDF samples this across the quad).
            let tl = (to_clip(q.x, q.y), ct, [u0, v0], [-ghw, -ghh]);
            let tr = (to_clip(q.x + q.w, q.y), ct, [u1, v0], [ghw, -ghh]);
            let bl = (to_clip(q.x, q.y + q.h), cb, [u0, v1], [-ghw, ghh]);
            let br = (to_clip(q.x + q.w, q.y + q.h), cb, [u1, v1], [ghw, ghh]);
            for (pos, color, uv, local) in [tl, bl, br, tl, br, tr] {
                verts.push(Vertex {
                    pos,
                    color,
                    uv,
                    local,
                    params,
                });
            }
        }
        let bytes = bytemuck::cast_slice(&verts);
        if bytes.len() as u64 > self.capacity {
            return; // over budget for this frame; the cap is generous
        }
        self.queue.write_buffer(&self.vertices, 0, bytes);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shell-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(PAPER),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.atlas_bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..verts.len() as u32, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}

/// A flattened, viewport-pixel quad (x right / y down) with an explicit composite
/// order band (chrome panels < board < text), used by the shell draw path. `color` is the
/// TOP colour and `color2` the BOTTOM colour — equal for a flat fill, distinct for a
/// vertical gradient (the quad pipeline interpolates per vertex, so a gradient is free).
/// `uv0`/`uv1` are the atlas UV rect: the solid-white texel for a SHAPE (a flat fill), or a
/// glyph's coverage cell for TEXT (anti-aliased serif type) — one pipeline draws both.
struct PxQuad {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: crate::scene::Color,
    color2: crate::scene::Color,
    uv0: (f32, f32),
    uv1: (f32, f32),
    order: u8,
    /// Corner radius in px for the rounded-box SDF. `< 0` ⇒ a plain textured quad (text + the
    /// board pass); `>= 0` ⇒ a rounded fill (or a soft shadow when `softness > 0`).
    radius: f32,
    /// Soft-shadow falloff band in px (0 ⇒ a crisp rounded fill; `> 0` ⇒ a soft drop shadow).
    softness: f32,
    /// Override SDF half-extents in px. `None` ⇒ derive from `w/h` (a rounded fill fills its
    /// quad). `Some((hw, hh))` ⇒ a SHADOW: the quad geometry is grown to give the penumbra
    /// room, but the SDF box stays the **caster's** size, so the soft falloff radiates from the
    /// caster's edge — not the grown geometry's.
    sdf_half: Option<(f32, f32)>,
}

impl PxQuad {
    /// A plain textured quad (the old path): a flat fill or a glyph, no SDF rounding.
    fn plain(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: crate::scene::Color,
        color2: crate::scene::Color,
        uv0: (f32, f32),
        uv1: (f32, f32),
        order: u8,
    ) -> PxQuad {
        PxQuad {
            x,
            y,
            w,
            h,
            color,
            color2,
            uv0,
            uv1,
            order,
            radius: -1.0,
            softness: 0.0,
            sdf_half: None,
        }
    }
}

fn err<E: std::fmt::Display>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}
