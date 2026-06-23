//! cardpipe — the Recollect card-art delivery pipeline.
//!
//! Cards render as text today; this tool prepares the per-card illustrations that
//! the catalog page (and, later, the wgpu client) reference by filename. It does
//! NOT paint art — the illustrations are made by an external AI image tool and
//! land as `assets/cards-src/<key>.png` masters. This tool does everything else:
//!
//!   * `placeholder` — rasterize the committed `assets/placeholder.svg` into a
//!     master PNG + the delivered WebP/JPEG set, so every card resolves an image
//!     even before any real art exists.
//!   * `optimize`    — for each card `key` in the catalog, take the real master
//!     (`assets/cards-src/<key>.png`) when present, else the placeholder, and emit
//!     `site/img/cards/<key>{-512.webp,-1024.webp,.webp,.jpg}` at the delivered
//!     widths, targeting the ~30–80 KB/image gzip budget.
//!   * `check`       — the gate: assert every deck-playable card resolves to a
//!     delivered image (real or the documented placeholder). Exit non-zero on a gap.
//!
//! Naming is the contract: the filename is the card's stable `key`. To add real
//! art for a card, drop `assets/cards-src/<key>.png` and re-run `make cards-images`.
//!
//! See `docs/decisions/card_images.md` for the full plan and the art prompt.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbaImage};

/// Delivered widths for the `srcset`. The master is 1024 wide (2× the on-screen
/// card); 512 covers 1× / small viewports.
const WIDTHS: &[u32] = &[512, 1024];
/// Lossy WebP quality (0–100). 80 keeps placeholders and most art comfortably
/// inside the ~30–80 KB target while staying crisp at card size.
const WEBP_QUALITY: f32 = 80.0;
/// JPEG fallback quality — the non-WebP escape hatch deliverable.
const JPEG_QUALITY: u8 = 82;
/// Master art dimensions (5:7 portrait), matching the external-tool output.
const MASTER_W: u32 = 1024;
const MASTER_H: u32 = 1434;
/// Reserved delivered key for the one shared placeholder set. Cards without real
/// art point here (the page generator emits this path); a real `<key>` never
/// collides because catalog keys are card-name slugs (no leading underscore).
const PLACEHOLDER_KEY: &str = "_placeholder";

/// Card kinds that go into a deck for *either* faction — the set the has-art gate
/// guards. Mirrors `CardKind::deck_playable_for` in recollect-core (Lorekeeper
/// spellbook ∪ Solace antagonist set). Kept as a literal list because this tool
/// is intentionally outside the workspace and must not depend on the engine.
const DECK_PLAYABLE_KINDS: &[&str] = &[
    // Lorekeeper deck (CardKind::deck_playable)
    "Spirit",
    "Caller",
    "Ritual",
    "Bond",
    "Landmark",
    "Fabrication",
    // Solace deck
    "Unwritten",
    "IllIntent",
    "Unwriting",
];

fn repo_root() -> PathBuf {
    // tools/cardpipe/ -> repo root is two parents up from the manifest dir.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("cardpipe lives at <root>/tools/cardpipe")
        .to_path_buf()
}

struct Paths {
    catalog: PathBuf,
    placeholder_svg: PathBuf,
    masters_dir: PathBuf,
    /// Rasterized placeholder master (generated, not committed).
    placeholder_png: PathBuf,
    out_dir: PathBuf,
}

impl Paths {
    fn new() -> Self {
        let root = repo_root();
        Paths {
            catalog: root.join("app/crates/recollect-core/data/catalog.json"),
            placeholder_svg: root.join("assets/placeholder.svg"),
            masters_dir: root.join("assets/cards-src"),
            placeholder_png: root.join("assets/cards-src/_placeholder.master.png"),
            out_dir: root.join("site/img/cards"),
        }
    }
}

/// Minimal catalog row — only the fields the pipeline needs (`key`/`name`/`kind`),
/// to keep this tool decoupled from the engine's full card schema.
struct Card {
    key: String,
    name: String,
    kind: String,
}

/// Parse just `key`, `name`, `kind` out of the generated catalog.json (a flat array
/// of card objects). Read-only — this tool never writes the catalog.
fn read_catalog(path: &Path) -> Result<Vec<Card>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read catalog {}: {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("parse catalog: {e}"))?;
    let arr = json.as_array().ok_or("catalog.json is not an array")?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let get = |k: &str| -> Result<String, String> {
            v.get(k)
                .and_then(|x| x.as_str())
                .map(str::to_owned)
                .ok_or_else(|| format!("card #{i} missing string field `{k}`"))
        };
        out.push(Card {
            key: get("key")?,
            name: get("name")?,
            kind: get("kind")?,
        });
    }
    Ok(out)
}

/// Rasterize the placeholder SVG into the master-resolution PNG.
fn render_placeholder(p: &Paths) -> Result<(), String> {
    let svg = std::fs::read(&p.placeholder_svg)
        .map_err(|e| format!("read {}: {e}", p.placeholder_svg.display()))?;

    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&svg, &opt)
        .map_err(|e| format!("parse placeholder.svg: {e}"))?;

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(MASTER_W, MASTER_H).ok_or("alloc placeholder pixmap")?;
    // Scale the SVG's intrinsic size to the master canvas (it is authored 1024×1434,
    // so this is identity today; the scale keeps us robust if the SVG is re-authored).
    let sz = tree.size();
    let sx = MASTER_W as f32 / sz.width();
    let sy = MASTER_H as f32 / sz.height();
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let img = RgbaImage::from_raw(MASTER_W, MASTER_H, pixmap.data().to_vec())
        .ok_or("placeholder pixmap -> image")?;
    if let Some(parent) = p.placeholder_png.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    img.save_with_format(&p.placeholder_png, ImageFormat::Png)
        .map_err(|e| format!("write {}: {e}", p.placeholder_png.display()))?;
    println!(
        "placeholder: rendered {} ({MASTER_W}×{MASTER_H})",
        rel(&p.placeholder_png)
    );
    Ok(())
}

/// Encode `img` to lossy WebP at the given width, writing `<stem>-<w>.webp` (and,
/// for the canonical width, also bare `<stem>.webp` used as the `<img src>`).
fn emit_webp(img: &DynamicImage, stem: &Path, width: u32, canonical: bool) -> Result<u64, String> {
    let resized = resize_to_width(img, width);
    let encoder =
        webp::Encoder::from_image(&resized).map_err(|e| format!("webp encoder ({width}w): {e}"))?;
    let mem = encoder.encode(WEBP_QUALITY);
    let bytes: &[u8] = &mem;

    let mut total = 0u64;
    let sized = with_suffix(stem, &format!("-{width}.webp"));
    std::fs::write(&sized, bytes).map_err(|e| format!("write {}: {e}", sized.display()))?;
    total += bytes.len() as u64;
    if canonical {
        let bare = with_suffix(stem, ".webp");
        std::fs::write(&bare, bytes).map_err(|e| format!("write {}: {e}", bare.display()))?;
        total += bytes.len() as u64;
    }
    Ok(total)
}

/// Resize preserving aspect; width-bound (height follows the 5:7 master).
fn resize_to_width(img: &DynamicImage, width: u32) -> DynamicImage {
    if img.width() == width {
        return img.clone();
    }
    let height = ((width as u64 * img.height() as u64) / img.width() as u64).max(1) as u32;
    img.resize_exact(width, height, FilterType::Lanczos3)
}

fn with_suffix(stem: &Path, suffix: &str) -> PathBuf {
    let mut s = stem.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Produce the full delivered set for one source image at `out_dir/<key>`:
/// `<key>-512.webp`, `<key>-1024.webp`, `<key>.webp` (canonical = src), `<key>.jpg`.
fn deliver(img: &DynamicImage, out_dir: &Path, key: &str) -> Result<u64, String> {
    let stem = out_dir.join(key);
    let mut total = 0u64;
    for &w in WIDTHS {
        let canonical = w == *WIDTHS.last().unwrap(); // largest width backs `src`
        total += emit_webp(img, &stem, w, canonical)?;
    }
    // JPEG fallback at the canonical width (the non-WebP escape-hatch deliverable).
    let fallback = resize_to_width(img, *WIDTHS.last().unwrap());
    let jpg = with_suffix(&stem, ".jpg");
    let mut buf = std::io::Cursor::new(Vec::new());
    fallback
        .to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut buf,
            JPEG_QUALITY,
        ))
        .map_err(|e| format!("jpeg encode {key}: {e}"))?;
    let jpg_bytes = buf.into_inner();
    std::fs::write(&jpg, &jpg_bytes).map_err(|e| format!("write {}: {e}", jpg.display()))?;
    total += jpg_bytes.len() as u64;
    Ok(total)
}

/// Load a master PNG (real art) into a DynamicImage.
fn load_master(path: &Path) -> Result<DynamicImage, String> {
    image::open(path).map_err(|e| format!("open master {}: {e}", path.display()))
}

fn cmd_optimize(p: &Paths) -> Result<(), String> {
    let cards = read_catalog(&p.catalog)?;
    // Rebuild the delivered set from scratch each run: it is fully regenerated from
    // the masters + placeholder, so a removed/renamed master leaves no orphaned
    // `<old-key>.webp` behind (which would mismatch the page's placeholder fallback).
    if p.out_dir.exists() {
        std::fs::remove_dir_all(&p.out_dir)
            .map_err(|e| format!("clean {}: {e}", p.out_dir.display()))?;
    }
    std::fs::create_dir_all(&p.out_dir)
        .map_err(|e| format!("mkdir {}: {e}", p.out_dir.display()))?;

    // The shared placeholder is delivered ONCE under the reserved `_placeholder`
    // key — never copied per-card. The page generator points every art-less card
    // at `_placeholder.webp`, so 407 placeholder cards cost four files, not 1628.
    // When a master lands, that card gets its own per-key set and the page switches.
    render_placeholder(p)?;
    let placeholder = load_master(&p.placeholder_png)?;
    let ph_bytes = deliver(&placeholder, &p.out_dir, PLACEHOLDER_KEY)?;

    // Per-key sets only for cards that actually have real master art today.
    let mut real = 0usize;
    let mut real_bytes = 0u64;
    let mut max_real_webp = 0u64;
    let mut over_budget: Vec<(String, u64)> = Vec::new();

    for c in &cards {
        let master = p.masters_dir.join(format!("{}.png", c.key));
        if !master.exists() {
            continue; // resolves to the shared placeholder via the page generator
        }
        let img = load_master(&master)?;
        real_bytes += deliver(&img, &p.out_dir, &c.key)?;
        real += 1;
        let canonical = p.out_dir.join(format!("{}.webp", c.key));
        if let Ok(meta) = std::fs::metadata(&canonical) {
            let sz = meta.len();
            max_real_webp = max_real_webp.max(sz);
            // 80 KB is the documented per-image ceiling; surfaced (not fatal) so the
            // client's ≤3 MB gzip budget (Trunk T-5) stays visible.
            if sz > 80 * 1024 {
                over_budget.push((c.key.clone(), sz));
            }
        }
    }

    let placeheld = cards.len() - real;
    let ph_canonical = std::fs::metadata(p.out_dir.join(format!("{PLACEHOLDER_KEY}.webp")))
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "optimize: {} cards -> {} ({real} with real master art, {placeheld} sharing the placeholder)",
        cards.len(),
        rel(&p.out_dir),
    );
    println!(
        "optimize: placeholder set {:.1} KB total (canonical webp {:.1} KB); real-art emitted {:.1} MB across all widths+fallback{}",
        ph_bytes as f64 / 1024.0,
        ph_canonical as f64 / 1024.0,
        real_bytes as f64 / (1024.0 * 1024.0),
        if real > 0 {
            format!(" (largest real webp {:.1} KB)", max_real_webp as f64 / 1024.0)
        } else {
            String::new()
        },
    );
    if !over_budget.is_empty() {
        eprintln!(
            "optimize: NOTE {} real-art image(s) exceed the 80 KB target (re-export the master smaller if the client budget tightens):",
            over_budget.len()
        );
        for (k, sz) in over_budget.iter().take(10) {
            eprintln!("    {k}.webp = {:.1} KB", *sz as f64 / 1024.0);
        }
    }
    Ok(())
}

fn cmd_placeholder(p: &Paths) -> Result<(), String> {
    render_placeholder(p)?;
    // Also emit the placeholder under its reserved delivered key so it can be
    // referenced/inspected directly and so `site/img/cards/` is never empty.
    std::fs::create_dir_all(&p.out_dir)
        .map_err(|e| format!("mkdir {}: {e}", p.out_dir.display()))?;
    let img = load_master(&p.placeholder_png)?;
    let bytes = deliver(&img, &p.out_dir, PLACEHOLDER_KEY)?;
    println!(
        "placeholder: delivered set written ({:.1} KB total) -> {}/{PLACEHOLDER_KEY}.*",
        bytes as f64 / 1024.0,
        rel(&p.out_dir)
    );
    Ok(())
}

/// The gate: every deck-playable card must resolve a delivered canonical WebP —
/// its own `<key>.webp` (real master present) or the shared `_placeholder.webp`.
fn cmd_check(p: &Paths) -> Result<bool, String> {
    let cards = read_catalog(&p.catalog)?;

    // The shared placeholder must exist for any art-less card to resolve.
    let placeholder_ok = p.out_dir.join(format!("{PLACEHOLDER_KEY}.webp")).exists();

    let mut missing: Vec<(&str, &str)> = Vec::new();
    let mut on_placeholder = 0usize;

    for c in &cards {
        if !DECK_PLAYABLE_KINDS.contains(&c.kind.as_str()) {
            continue;
        }
        let has_master = p.masters_dir.join(format!("{}.png", c.key)).exists();
        if has_master {
            // Real art: its per-key delivered WebP must have been emitted.
            if !p.out_dir.join(format!("{}.webp", c.key)).exists() {
                missing.push((&c.key, &c.name));
            }
        } else if placeholder_ok {
            on_placeholder += 1;
        } else {
            // No master AND no placeholder built — this card resolves nothing.
            missing.push((&c.key, &c.name));
        }
    }

    let total = cards
        .iter()
        .filter(|c| DECK_PLAYABLE_KINDS.contains(&c.kind.as_str()))
        .count();

    if missing.is_empty() {
        println!(
            "cardpipe check OK: all {total} deck-playable cards resolve a delivered image \
             ({} real master art, {on_placeholder} via the shared placeholder).",
            total - on_placeholder
        );
        Ok(true)
    } else {
        if !placeholder_ok {
            eprintln!("cardpipe check FAILED: the shared placeholder (site/img/cards/{PLACEHOLDER_KEY}.webp) is missing.");
        }
        eprintln!(
            "cardpipe check FAILED: {} of {total} deck-playable cards resolve no delivered image.",
            missing.len()
        );
        eprintln!(
            "Run `make cards-images` to build placeholders (and real art where masters exist)."
        );
        for (k, name) in missing.iter().take(20) {
            eprintln!("    unresolved: {k}  ({name})");
        }
        Ok(false)
    }
}

fn rel(path: &Path) -> String {
    let root = repo_root();
    path.strip_prefix(&root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn run() -> Result<bool, String> {
    let p = Paths::new();
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "placeholder" => cmd_placeholder(&p).map(|_| true),
        "optimize" => cmd_optimize(&p).map(|_| true),
        "check" => cmd_check(&p),
        "all" | "" => {
            // Default: optimize (which renders + delivers the shared placeholder and
            // every real master), then gate. No separate placeholder pass needed.
            cmd_optimize(&p)?;
            cmd_check(&p)
        }
        other => Err(format!(
            "unknown command `{other}`. Usage: cardpipe [placeholder|optimize|check|all]"
        )),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("cardpipe: {e}");
            ExitCode::FAILURE
        }
    }
}
