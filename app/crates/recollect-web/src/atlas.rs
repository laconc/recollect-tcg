//! A **glyph atlas** for real, anti-aliased serif type in the canvas (wasm-only).
//!
//! The renderer is one alpha-blended, *textured* quad pipeline (see [`render`](crate::render)).
//! Rather than draw text from a 3×5 bitmap (blocky, "8-bit"), this module rasterizes the
//! bundled **EB Garamond** (OFL-1.1, `assets/EBGaramond.ttf` — a classic literary Garamond that
//! matches the website's storybook register) into a single coverage texture at startup with the
//! pure-Rust [`ab_glyph`] rasterizer — no system fonts (there are none on
//! `wasm32-unknown-unknown`), no font-server, no DOM text. The same atlas would build natively
//! from the same bytes, so the canvas stays *one surface across platforms* (the design-of-record's
//! shared-renderer commitment).
//!
//! The atlas packs a curated glyph set (ASCII printable + the typographic marks the UI uses) into
//! rows, plus a 2×2 **solid white texel** (`solid_uv`) so the *shape* quads can share the exact
//! same textured pipeline — a fill is just a textured quad sampling the white texel, a glyph is a
//! textured quad sampling its cell. One pipeline, one draw.
#![cfg(target_arch = "wasm32")]

use ab_glyph::{Font, FontRef, Glyph, PxScale, ScaleFont};

/// The bundled serif (EB Garamond, OFL-1.1 — see `assets/OFL.txt`). Real anti-aliased type.
const FONT_BYTES: &[u8] = include_bytes!("../assets/EBGaramond.ttf");

/// The pixel height the glyphs are rasterized at in the atlas. Generous, so scaling the cell
/// DOWN to a label's on-screen size stays crisp (we never scale a glyph UP past this). 64px is a
/// good crispness/atlas-size trade for the largest type (the result-screen verdict, the names).
const RASTER_PX: f32 = 64.0;
/// Padding around each glyph cell (avoids bilinear bleed between neighbours).
const PAD: u32 = 2;

/// One rasterized glyph's placement in the atlas + its layout metrics (all in the atlas's
/// RASTER_PX space; the caller scales by `target_px / RASTER_PX`).
#[derive(Clone, Copy)]
struct GlyphInfo {
    /// Atlas pixel rect of the coverage bitmap.
    ax: u32,
    ay: u32,
    aw: u32,
    ah: u32,
    /// Left/top bearing from the pen origin/baseline to the bitmap's top-left.
    bx: f32,
    by: f32,
    /// Horizontal advance (pen step).
    advance: f32,
}

/// A positioned textured quad for one glyph — viewport px (relative to the layout origin the
/// caller passes) + the atlas UVs. The backend maps these straight into its vertex buffer.
#[derive(Clone, Copy)]
pub struct GlyphQuad {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// The built atlas: the packed coverage texture (R8, one byte = alpha) + the per-glyph table and
/// the font's vertical metrics, plus the white-texel UV the shape quads sample.
pub struct GlyphAtlas {
    pub width: u32,
    pub height: u32,
    /// R8 coverage (alpha) bytes, row-major, `width*height` long.
    pub pixels: Vec<u8>,
    glyphs: std::collections::HashMap<char, GlyphInfo>,
    /// The font's ascent/line metrics at RASTER_PX (for vertical centring).
    ascent: f32,
    descent: f32,
    /// UV of the centre of the solid-white texel (shape quads sample this for a flat fill).
    pub solid_uv: (f32, f32),
}

/// The glyph set the UI needs: ASCII printable (names, stats, rules, buttons) plus the
/// typographic marks the design uses (em dash, middot, en dash, curly quotes, ellipsis).
fn glyph_set() -> Vec<char> {
    let mut v: Vec<char> = (0x20u8..0x7f).map(|b| b as char).collect();
    v.extend(['—', '·', '–', '’', '‘', '“', '”', '…', '×', '•']);
    v
}

impl GlyphAtlas {
    /// Rasterize the bundled font into a coverage atlas. Pure CPU; called once at renderer start.
    pub fn build() -> GlyphAtlas {
        let font = FontRef::try_from_slice(FONT_BYTES).expect("bundled EB Garamond parses");
        let scaled = font.as_scaled(PxScale::from(RASTER_PX));
        let ascent = scaled.ascent();
        let descent = scaled.descent();

        let chars = glyph_set();
        // Rasterize each glyph to a coverage bitmap, collecting cells to pack.
        struct Cell {
            ch: char,
            w: u32,
            h: u32,
            cov: Vec<u8>,
            bx: f32,
            by: f32,
            advance: f32,
        }
        let mut cells: Vec<Cell> = Vec::with_capacity(chars.len());
        for ch in chars {
            let gid = font.glyph_id(ch);
            let advance = scaled.h_advance(gid);
            let glyph: Glyph = gid.with_scale(PxScale::from(RASTER_PX));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bb = outline.px_bounds();
                let w = bb.width().ceil().max(1.0) as u32;
                let h = bb.height().ceil().max(1.0) as u32;
                let mut cov = vec![0u8; (w * h) as usize];
                outline.draw(|gx, gy, c| {
                    if gx < w && gy < h {
                        cov[(gy * w + gx) as usize] = (c * 255.0) as u8;
                    }
                });
                cells.push(Cell {
                    ch,
                    w,
                    h,
                    cov,
                    bx: bb.min.x,
                    by: bb.min.y,
                    advance,
                });
            } else {
                // A blank glyph (space) — record its advance, no bitmap.
                cells.push(Cell {
                    ch,
                    w: 0,
                    h: 0,
                    cov: Vec::new(),
                    bx: 0.0,
                    by: 0.0,
                    advance,
                });
            }
        }

        // Simple shelf packer: a fixed-width atlas, rows growing downward. Reserve the first row's
        // start for a 2×2 solid-white texel the shape quads sample.
        let atlas_w: u32 = 1024;
        let mut x = PAD;
        let mut y = PAD;
        let mut row_h = 0u32;
        // The solid texel first.
        let solid = 2u32;
        let solid_x = x;
        let solid_y = y;
        x += solid + PAD;
        row_h = row_h.max(solid);

        let mut glyphs = std::collections::HashMap::new();
        let mut placements: Vec<(usize, u32, u32)> = Vec::with_capacity(cells.len());
        for (i, cell) in cells.iter().enumerate() {
            if cell.w == 0 {
                glyphs.insert(
                    cell.ch,
                    GlyphInfo {
                        ax: 0,
                        ay: 0,
                        aw: 0,
                        ah: 0,
                        bx: cell.bx,
                        by: cell.by,
                        advance: cell.advance,
                    },
                );
                continue;
            }
            if x + cell.w + PAD > atlas_w {
                // New shelf.
                x = PAD;
                y += row_h + PAD;
                row_h = 0;
            }
            placements.push((i, x, y));
            glyphs.insert(
                cell.ch,
                GlyphInfo {
                    ax: x,
                    ay: y,
                    aw: cell.w,
                    ah: cell.h,
                    bx: cell.bx,
                    by: cell.by,
                    advance: cell.advance,
                },
            );
            x += cell.w + PAD;
            row_h = row_h.max(cell.h);
        }
        let atlas_h = (y + row_h + PAD).next_power_of_two();

        // Blit the coverage bitmaps into the atlas, and write the solid texel (full coverage).
        let mut pixels = vec![0u8; (atlas_w * atlas_h) as usize];
        for dy in 0..solid {
            for dx in 0..solid {
                pixels[((solid_y + dy) * atlas_w + (solid_x + dx)) as usize] = 255;
            }
        }
        for (i, px, py) in placements {
            let cell = &cells[i];
            for cy in 0..cell.h {
                for cx in 0..cell.w {
                    let v = cell.cov[(cy * cell.w + cx) as usize];
                    pixels[((py + cy) * atlas_w + (px + cx)) as usize] = v;
                }
            }
        }

        let solid_uv = (
            (solid_x as f32 + solid as f32 * 0.5) / atlas_w as f32,
            (solid_y as f32 + solid as f32 * 0.5) / atlas_h as f32,
        );

        GlyphAtlas {
            width: atlas_w,
            height: atlas_h,
            pixels,
            glyphs,
            ascent,
            descent,
            solid_uv,
        }
    }

    /// The pixel width a string occupies at `target_px` glyph height — for centring/right-aligning
    /// (mirrors [`crate::font::text_width`] so the shell's `fit_size` math still holds).
    pub fn text_width(&self, text: &str, target_px: f32) -> f32 {
        let s = target_px / RASTER_PX;
        let mut w = 0.0f32;
        for ch in text.chars() {
            if let Some(g) = self.glyphs.get(&ch) {
                w += g.advance * s;
            } else {
                w += RASTER_PX * 0.5 * s; // a sane fallback advance for an unknown glyph
            }
        }
        w
    }

    /// Lay out `text` so it is **centred horizontally on `cx`** and **vertically on `cy`**, at glyph
    /// height `target_px` — the same anchoring the shell's [`Text`](crate::shell::Text) primitives
    /// use. Returns one textured quad per visible glyph (spaces emit nothing).
    pub fn layout_centered(&self, cx: f32, cy: f32, target_px: f32, text: &str) -> Vec<GlyphQuad> {
        let s = target_px / RASTER_PX;
        let total_w = self.text_width(text, target_px);
        let mut pen_x = cx - total_w / 2.0;
        // Centre the cap-height-ish band on cy: use ascent+descent as the line box.
        let line = (self.ascent - self.descent) * s;
        let baseline = cy + line / 2.0 + self.descent * s;
        let mut out = Vec::new();
        for ch in text.chars() {
            let Some(g) = self.glyphs.get(&ch) else {
                pen_x += RASTER_PX * 0.5 * s;
                continue;
            };
            if g.aw > 0 {
                let gx = pen_x + g.bx * s;
                let gy = baseline + g.by * s;
                let gw = g.aw as f32 * s;
                let gh = g.ah as f32 * s;
                let u0 = g.ax as f32 / self.width as f32;
                let v0 = g.ay as f32 / self.height as f32;
                let u1 = (g.ax + g.aw) as f32 / self.width as f32;
                let v1 = (g.ay + g.ah) as f32 / self.height as f32;
                out.push(GlyphQuad {
                    x: gx,
                    y: gy,
                    w: gw,
                    h: gh,
                    u0,
                    v0,
                    u1,
                    v1,
                });
            }
            pen_x += g.advance * s;
        }
        out
    }
}
