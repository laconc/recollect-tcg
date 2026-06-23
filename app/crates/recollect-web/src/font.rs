//! A tiny 3×5 bitmap font's **metric**, used now only as a pure layout estimate.
//!
//! The canvas renders real anti-aliased serif type from a glyph atlas (the wasm-only
//! `atlas` module) — this bitmap is no longer the renderer's glyph source. What survives is
//! [`text_width`]: the **pure** (native-testable, no-wgpu) width estimate the shell's layout
//! math (`fit_size`, the replay-caption chip) calls to size/centre labels before they're
//! handed to the atlas to draw. Garamond renders narrower than this fixed cell, so the shell
//! scales the estimate (`shell::FONT_WIDTH_RATIO`); the actual on-screen alignment uses the
//! atlas's own width. [`emit`] (the per-pixel rasterizer) is retained for its tests.
use crate::scene::{Color, Layer, Quad};

/// Rows top→bottom; the low 3 bits of each byte are the columns, bit 2 = leftmost.
fn glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '?' => [0b111, 0b001, 0b011, 0b000, 0b010],
        // Uppercase A–Z (for short card names on the board).
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b111, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        // Lowercase the terrain tags use ("Lm", "Fb") have their own forms.
        'b' => [0b100, 0b100, 0b111, 0b101, 0b111],
        'm' => [0b000, 0b111, 0b111, 0b101, 0b101],
        ' ' => [0, 0, 0, 0, 0],
        // Any other lowercase letter borrows its uppercase glyph (the 3×5 cell has
        // no room for distinct lowercase forms). This keeps mixed-case UI text — the
        // shell's "End Turn", "Round 9 · Dusk", the opponent name — legible instead
        // of rendering as boxes; the only dedicated lowercase glyphs are b/m above.
        c if c.is_ascii_lowercase() => glyph(c.to_ascii_uppercase()),
        _ => [0b111, 0b101, 0b101, 0b101, 0b111], // unknown → filled box
    }
}

const COLS: usize = 3;
const ROWS: usize = 5;

/// The tile-grid width a string occupies at glyph height `h` (so callers can
/// center it). Glyphs are 3 wide with a 1-pixel gap.
pub fn text_width(text: &str, h: f32) -> f32 {
    let px = h / ROWS as f32;
    let n = text.chars().count() as f32;
    (n * (COLS as f32 + 1.0) - 1.0).max(0.0) * px
}

/// Rasterize `text` centered horizontally on `cx`, vertically on `cy`, at glyph
/// height `h` (tile-grid units), appending one quad per lit pixel.
pub fn emit(out: &mut Vec<Quad>, cx: f32, cy: f32, h: f32, text: &str, color: Color) {
    let px = h / ROWS as f32;
    let mut x = cx - text_width(text, h) / 2.0;
    let top = cy - h / 2.0;
    for ch in text.chars() {
        let g = glyph(ch);
        for (r, row) in g.iter().enumerate() {
            for c in 0..COLS {
                if row & (1 << (COLS - 1 - c)) != 0 {
                    out.push(Quad {
                        x: x + c as f32 * px,
                        y: top + r as f32 * px,
                        w: px,
                        h: px,
                        color,
                        layer: Layer::Marker,
                    });
                }
            }
        }
        x += (COLS as f32 + 1.0) * px;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_lit_pixel_becomes_a_quad() {
        // "8" lights 13 of its 15 pixels; rasterizing it yields one quad each.
        let mut out = Vec::new();
        emit(
            &mut out,
            0.5,
            0.5,
            0.25,
            "8",
            Color::rgba(1.0, 1.0, 1.0, 1.0),
        );
        let lit: u32 = super::glyph('8').iter().map(|r| r.count_ones()).sum();
        assert_eq!(out.len() as u32, lit);
        assert!(out.iter().all(|q| q.layer == Layer::Marker));
    }

    #[test]
    fn a_two_digit_number_is_wider_than_one() {
        assert!(text_width("42", 0.25) > text_width("4", 0.25));
    }

    #[test]
    fn a_space_draws_nothing() {
        let mut out = Vec::new();
        emit(
            &mut out,
            0.5,
            0.5,
            0.25,
            " ",
            Color::rgba(1.0, 1.0, 1.0, 1.0),
        );
        assert!(out.is_empty());
    }
}
