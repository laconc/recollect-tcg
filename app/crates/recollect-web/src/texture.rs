//! Per-card art textures for the wgpu board (wasm-only). Each card's illustration
//! is delivered by a parallel pipeline as `img/cards/<key>-512.webp` (the stem is
//! the card's frozen slug — see `docs/decisions/card_images.md`); this module
//! loads them **lazily** (only when a card first appears on the board), keyed by
//! that `key`, and hands the backend a sampled-texture bind group per card face.
//!
//! **Decode path — the browser, not Rust.** WebP→pixels goes through the browser's
//! own hardware decoder (`fetch` → `Blob` → `createImageBitmap`), then
//! [`wgpu::Queue::copy_external_image_to_texture`] uploads the `ImageBitmap`
//! straight to GPU memory. Bundling a Rust WebP decoder would add hundreds of KB
//! to the wasm for a job the browser already does for free; this keeps the ≤3 MB
//! gzip budget (Trunk red-team T-5) essentially flat. The upload respects the
//! WebGL2-fallback constraints (zero origin, srgb dest, `flip_y`/`premultiplied`
//! both false) so the same path works with or without WebGPU.
//!
//! **Graceful fallback.** A key with no delivered art (404) → the shared
//! `_placeholder` texture; the placeholder itself absent → no bind group at all,
//! and the seat-ink card frame from `scene.rs` simply shows through. The client
//! therefore builds and renders with **zero real art present** — the launch state.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, ImageBitmap, Response};

/// Where the pipeline delivers art, and which width we pull. The on-screen card
/// cell is ~60–360 px, so the 512w master is ample and the 1024w would only cost
/// bandwidth. The shared art-less stand-in is `_placeholder` (same naming as
/// `tools/cardpipe`). Root-relative (`/img/cards`) so it resolves to the shared
/// site origin where the card art lives, whether the play page is served at `/`
/// or a subpath (the launch plan fronts site + client + assets on one origin via
/// the Cloudflare CDN — see docs/decisions/playtest_launch_plan.md).
const IMG_BASE: &str = "/img/cards";
const DELIVERED_WIDTH: u32 = 512;
const PLACEHOLDER_KEY: &str = "_placeholder";

/// The delivered URL for an image stem at the width we sample.
fn art_url(stem: &str) -> String {
    format!("{IMG_BASE}/{stem}-{DELIVERED_WIDTH}.webp")
}

/// Per-key load state. A texture is requested at most once; the result is cached
/// (including failure, so a 404 isn't retried every frame).
enum LoadState {
    /// Fetch + decode in flight; nothing to draw yet (placeholder shows).
    Pending,
    /// Decoded + uploaded; ready to sample.
    Ready(Rc<wgpu::BindGroup>),
    /// 404 / decode error — fall back to the placeholder for good.
    Failed,
}

/// A decoded bitmap waiting on the render thread to be uploaded to a texture.
/// (wasm is single-threaded; the decode future can't touch the GPU device, so it
/// drops its result here and [`ArtTextures::pump`] uploads it next frame.)
struct Decoded {
    key: String,
    /// `Some` once the browser decoded it; `None` on fetch/decode failure.
    bitmap: Option<ImageBitmap>,
}

/// The card-art texture cache: a bind-group-layout + sampler shared with the
/// backend's art pipeline, the per-key load states, and a shared inbox the decode
/// futures push their results into.
pub struct ArtTextures {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    states: HashMap<String, LoadState>,
    /// Shared with the spawned decode futures (single-threaded; `Rc`, not `Arc`).
    inbox: Rc<RefCell<Vec<Decoded>>>,
}

impl ArtTextures {
    /// Build the cache, install a synthetic paper-&-ink placeholder (so faces show
    /// *something* even with zero art files — the launch/demo state), and kick off
    /// the shared `_placeholder.webp` load (which, if present, supersedes the
    /// synthetic one). The art texture format is linear `Rgba8Unorm` so the WebP's
    /// encoded bytes pass to the non-srgb surface unchanged — matching what an
    /// `<img>` would show.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> ArtTextures {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("card-art-bgl"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("card-art-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let mut me = ArtTextures {
            layout,
            sampler,
            format: wgpu::TextureFormat::Rgba8Unorm,
            states: HashMap::new(),
            inbox: Rc::new(RefCell::new(Vec::new())),
        };
        // The synthetic placeholder is ready immediately — the board never waits on
        // a network round-trip to show a framed illustration area, and the build
        // renders with no art files at all.
        let synthetic = me.synthetic_placeholder(device, queue);
        me.states
            .insert(PLACEHOLDER_KEY.to_string(), LoadState::Ready(Rc::new(synthetic)));
        // Still try the delivered placeholder; if it loads it replaces the synthetic.
        me.fetch(PLACEHOLDER_KEY);
        me
    }

    /// A small procedural paper-&-ink swatch as the always-available placeholder: a
    /// soft top-to-bottom wash from warm paper to a faint ink tint, so an art-less
    /// card reads as "illustration pending" rather than a flat block. Uploaded via
    /// `write_texture` — no image file, a few hundred bytes of pixels.
    fn synthetic_placeholder(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> wgpu::BindGroup {
        const W: u32 = 5;
        const H: u32 = 7; // the card-art 5:7 aspect
        let mut pixels = Vec::with_capacity((W * H * 4) as usize);
        for row in 0..H {
            let t = row as f32 / (H - 1) as f32; // 0 at top → 1 at bottom
            // Paper (0.96, 0.94, 0.89) lightly veiled toward ink (0.20, 0.22, 0.28).
            let mix = 0.10 + 0.14 * t;
            let chan = |paper: f32, ink: f32| ((paper * (1.0 - mix) + ink * mix) * 255.0) as u8;
            let (r, g, b) = (chan(0.96, 0.20), chan(0.94, 0.22), chan(0.89, 0.28));
            for _ in 0..W {
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let size = wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("card-art-placeholder"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(W * 4),
                rows_per_image: Some(H),
            },
            size,
        );
        self.bind_group_for_view(device, &texture.create_view(&Default::default()))
    }

    /// The bind group layout the art render pipeline binds against.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Ensure a load is in flight for `key` (idempotent — a key is fetched at most
    /// once). Call when a card first appears on the board; nothing happens for a key
    /// already loading/ready/failed.
    pub fn request(&mut self, key: &str) {
        if self.states.contains_key(key) {
            return;
        }
        self.states.insert(key.to_string(), LoadState::Pending);
        self.fetch(key);
    }

    /// Spawn the browser fetch+decode for `key`, dropping the result into the inbox
    /// for the next [`pump`](Self::pump). (Used by `request`, and directly for the
    /// delivered placeholder whose state slot is pre-seeded with the synthetic one.)
    fn fetch(&self, key: &str) {
        let key_owned = key.to_string();
        let inbox = self.inbox.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bitmap = decode(&art_url(&key_owned)).await.ok();
            inbox.borrow_mut().push(Decoded {
                key: key_owned,
                bitmap,
            });
        });
    }

    /// Drain decoded bitmaps into GPU textures + bind groups (called once per frame
    /// on the render thread, where the device/queue are available). Cheap when the
    /// inbox is empty. A failed delivered-placeholder fetch is ignored — the
    /// synthetic placeholder stays — so a 404 on `_placeholder.webp` doesn't blank
    /// the fallback.
    pub fn pump(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let pending: Vec<Decoded> = self.inbox.borrow_mut().drain(..).collect();
        for d in pending {
            match d.bitmap {
                Some(bmp) => {
                    let bg = LoadState::Ready(Rc::new(self.upload(device, queue, &bmp)));
                    self.states.insert(d.key, bg);
                }
                None if d.key == PLACEHOLDER_KEY => { /* keep the synthetic placeholder */ }
                None => {
                    self.states.insert(d.key, LoadState::Failed);
                }
            }
        }
    }

    /// The bind group to sample for `key`: its own art if ready, else the shared
    /// placeholder if ready, else `None` (draw nothing — the ink frame shows).
    /// Marks `key` for loading if it hasn't been requested yet, so merely asking
    /// to draw a card lazily triggers its fetch.
    pub fn bind_group(&mut self, key: &str) -> Option<Rc<wgpu::BindGroup>> {
        if !self.states.contains_key(key) {
            self.request(key);
        }
        if let Some(LoadState::Ready(bg)) = self.states.get(key) {
            return Some(bg.clone());
        }
        // Fall back to the placeholder while pending, or permanently on failure.
        match self.states.get(PLACEHOLDER_KEY) {
            Some(LoadState::Ready(bg)) => Some(bg.clone()),
            _ => None,
        }
    }

    /// Upload one decoded bitmap to a fresh texture and build its bind group. The
    /// copy respects the WebGL2 constraints (zero origin, srgb dest space,
    /// `flip_y` + `premultiplied_alpha` both false) so it works without WebGPU too.
    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bitmap: &ImageBitmap,
    ) -> wgpu::BindGroup {
        let (w, h) = (bitmap.width().max(1), bitmap.height().max(1));
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("card-art"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.copy_external_image_to_texture(
            &wgpu::CopyExternalImageSourceInfo {
                source: wgpu::ExternalImageSource::ImageBitmap(bitmap.clone()),
                origin: wgpu::Origin2d::ZERO,
                flip_y: false,
            },
            wgpu::CopyExternalImageDestInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
                color_space: wgpu::PredefinedColorSpace::Srgb,
                premultiplied_alpha: false,
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // The bitmap is decoded into the GPU texture now; release its CPU copy.
        bitmap.close();
        self.bind_group_for_view(device, &view)
    }

    /// A texture+sampler bind group over `view`, against the shared art layout.
    fn bind_group_for_view(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("card-art-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}

/// Fetch `url` and decode it via the browser's image decoder. `Err(())` on a
/// non-200, a missing `window`, or any decode failure — the caller treats that as
/// "no art" and falls back. The error payload is deliberately unit: nothing
/// actionable, and the placeholder is the recovery either way.
async fn decode(url: &str) -> Result<ImageBitmap, ()> {
    let window = web_sys::window().ok_or(())?;
    let resp_val = JsFuture::from(window.fetch_with_str(url)).await.map_err(drop)?;
    let resp: Response = resp_val.dyn_into().map_err(drop)?;
    if !resp.ok() {
        return Err(()); // 404 (no art delivered for this key) etc.
    }
    let blob_val = JsFuture::from(resp.blob().map_err(drop)?).await.map_err(drop)?;
    let blob: Blob = blob_val.dyn_into().map_err(drop)?;
    let bmp_promise = window.create_image_bitmap_with_blob(&blob).map_err(drop)?;
    let bmp_val = JsFuture::from(bmp_promise).await.map_err(drop)?;
    bmp_val.dyn_into::<ImageBitmap>().map_err(drop)
}
