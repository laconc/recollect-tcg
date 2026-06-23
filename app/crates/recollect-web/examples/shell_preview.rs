//! A **native, GPU-free preview rasterizer** for the in-canvas shell (and the board scene),
//! so the visual-polish work can be eyeballed deterministically when a headed GPU surface
//! isn't available. It builds a `ShellModel` from a deterministic local game (placing a few
//! spirits so the board cards show), lays it out with [`recollect_web::shell::build_shell`],
//! and rasterizes the resulting [`ShellScene`] to a PNG on the CPU — replicating the wgpu
//! backend's compositing: the layer order, the **rounded-box SDF** fills, the **soft drop
//! shadows**, the vertical **gradients**, and the **EB Garamond** atlas glyphs. It is a faithful
//! twin of `render.rs::draw_shell_scene`, not the GPU path itself — close enough to judge the
//! palette, the shadows, the card treatment, and the layout.
//!
//! Run, e.g.:
//!   cargo run -p recollect-web --example shell_preview -- /tmp/preview rest 1280 900
//!   cargo run -p recollect-web --example shell_preview -- /tmp/preview placed 1280 900
//!   cargo run -p recollect-web --example shell_preview -- /tmp/preview phone 412 915
//!
//! The args are: <out_prefix> <scenario> <width> <height>. Scenario ∈ {rest, placed, lifted,
//! inspect, special, 2v2, online-1v1, online-2v2, glimpse-burn, glimpse-keep, mulligan}. Writes
//! `<out_prefix>.png` (the gallery script passes the committed path as <out_prefix>). The Glimpse +
//! Mulligan trio render the in-canvas choice modals; `online-1v1` / `online-2v2` render the
//! FULL canvas shell for an ONLINE telling, built from the server's REDACTED view (no engine).

use ab_glyph::{Font, FontRef, Glyph, PxScale, ScaleFont};
use recollect_web::scene::Color as SColor;
use recollect_web::shell::{
    Align, GradRect, Rect, Shadow, ShellLayer, ShellModel, ShellScene, Text, build_shell,
    place_board,
};

const FONT_BYTES: &[u8] = include_bytes!("../assets/EBGaramond.ttf");
const RASTER_PX: f32 = 64.0;

fn main() {
    // Args: <scenario> <out_path.png> <width> <height>.
    let args: Vec<String> = std::env::args().collect();
    let scenario = args.get(1).cloned().unwrap_or_else(|| "rest".into());
    let path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| format!("/tmp/preview-{scenario}.png"));
    let vw: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1280.0);
    let vh: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(900.0);

    // Scenario routing:
    //  • `2v2`         — the bare 6×6 TeamView BOARD full-frame (the legacy local-2v2-WATCH look).
    //  • `online-1v1`  — the FULL canvas shell for an ONLINE 1v1 telling, built from the server's
    //                    REDACTED PlayerView + legal list (no engine) via `online::shell_model_for_*`
    //                    — the same shell a local telling draws, the opponent counts/backs only.
    //  • `online-2v2`  — the FULL canvas shell for an ONLINE 2v2 telling over the 6×6 TeamView.
    //  • `special`     — the tricky board states (items 12–14): a Landmark with a spirit, a held
    //                    lamplit spirit on a faded Dusk tile that can still act.
    //  • everything else — the full 1v1 LOCAL shell.
    let scene = if scenario == "2v2" {
        build_team_board_scene(vw, vh)
    } else if scenario == "online-1v1" {
        build_shell(&build_online_model(), vw, vh)
    } else if scenario == "online-2v2" {
        build_shell(&build_online_team_model(), vw, vh)
    } else if scenario == "special" {
        build_shell(&build_special_model(), vw, vh)
    } else {
        let model = build_model(&scenario);
        build_shell(&model, vw, vh)
    };
    let img = rasterize(&scene, vw as usize, vh as usize);
    write_png(&path, &img, vw as usize, vh as usize);
    eprintln!("wrote {path} ({}×{})", vw as u32, vh as u32);
}

/// Build a ShellModel with the TRICKY board states (items 12–14): a Landmark terrain that also
/// holds a spirit (the layering reads — the piece over the terrain), a held spirit lamplit on a
/// faded Dusk tile that can STILL act (the lamp glow + the action dot both legible), plus a couple
/// of ordinary spirits. Crafted directly on a core `Engine` (the test-support board mutators), then
/// assembled into a ShellModel the same way `shell_model_json` does (scores/round/affordances).
fn build_special_model() -> ShellModel {
    use recollect_core::cards::canon_catalog;
    use recollect_core::test_support::put_spirit;
    use recollect_core::types::{CardId, CardKind, Seat};
    use recollect_core::view::view_for;
    use recollect_core::{Engine, state::Terrain, state::TerrainKind};
    use recollect_web::scene::short_board_name;
    use recollect_web::shell::{HandCard, ShellModel};

    let cat = canon_catalog();
    let id_of = |name: &str| cat.iter().find(|c| c.name == name).map(|c| c.id);
    // A spirit deck so the opening hand is real cards.
    let deck: Vec<CardId> = cat
        .iter()
        .filter(|c| c.kind == CardKind::Spirit)
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut e, _) = Engine::new(7, cat.clone(), deck.clone(), deck);
    let cloud = id_of("Cloudling").unwrap_or(CardId(0));
    {
        let st = e.state_mut_for_test();
        // Item 12: a Landmark (seat A) on tile 12, WITH a spirit standing on it — the layering must
        // read (the spirit card over the terrain plate).
        st.board[12].terrain = Some(Terrain {
            card: cloud,
            owner: Seat::A,
            kind: TerrainKind::Landmark,
            face_down: false,
        });
        put_spirit(st, 12, cloud, Seat::A);
        // Item 13: a held spirit on a FADED (Dusk-dark) tile that can still act — the lamp glow
        // under it + the action dot both legible against the night.
        put_spirit(st, 7, cloud, Seat::A);
        st.board[7].faded = true;
        // §5 devolution: a STANDING-FADED form (Stormswell, a Primal banished in combat) on tile
        // 6 — it renders with the warm amber RESCUE GLOW + the downward recede chevron, and the
        // base "Cloudling" in hand can recede it. So the gallery still showcases the recede
        // affordance + the standing-Faded treatment alongside evolve / Dusk-held / landmark.
        let storm = id_of("Stormswell").unwrap_or(cloud);
        put_spirit(st, 6, storm, Seat::A);
        if let Some(sp) = st.board[6].spirit.as_mut() {
            sp.hp = 5; // wounded — the rescue would restore full HP
            sp.fading = true;
            sp.banished_by = Some(Seat::B);
            sp.fade_deadline = Some(st.round + 1); // the §0.5 rescue window
        }
        // An enemy spirit + one more ally, for a populated board.
        put_spirit(st, 18, cloud, Seat::B);
        put_spirit(st, 16, cloud, Seat::A);
    }
    let view = view_for(&e, Seat::A);
    let board_names: Vec<String> = cat.iter().map(|c| short_board_name(&c.name)).collect();
    let hand: Vec<HandCard> = view
        .you
        .hand
        .iter()
        .map(|id| {
            let c = e.card(*id);
            HandCard {
                name: c.name.clone(),
                cost: c.cost,
                attack: c.attack,
                defense: c.defense,
                hp: c.hp,
                kind: format!("{:?}", c.kind),
                resonance: format!("{:?}", c.resonance),
            }
        })
        .collect();
    // The recede BASE in hand (a Cloudling) for the standing-Faded form on tile 6, if the
    // opening hand holds one — so the devolve chevron shows on the card too (computed before
    // `view` is moved into the model below).
    let recede_base: Option<u8> = view
        .you
        .hand
        .iter()
        .position(|c| *c == cloud)
        .map(|i| i as u8);
    let mut actionable_hand = vec![0u8, 1];
    if let Some(i) = recede_base
        && !actionable_hand.contains(&i)
    {
        actionable_hand.push(i);
    }
    ShellModel {
        you_seat: Seat::A,
        you_name: "WARDEN AMES".into(),
        you_faction: "Lorekeepers".into(),
        you_score: 4,
        you_anima: 3,
        hand,
        opp_name: "CORIN ASHE".into(),
        opp_faction: "the Solace".into(),
        opp_score: 3,
        opp_erasures: 1,
        opp_hand_count: 4,
        round: 9, // past the Dusk (8) — the board has begun contracting, faded tiles exist
        last_round: recollect_core::engine::LAST_ROUND,
        dusk_after: e.state().rules.contraction_after,
        your_turn: true,
        view,
        names: board_names,
        cues: Default::default(),
        interaction: Default::default(),
        // Item 13/12: the held faded spirit (7) and the landmark-bearing spirit (12) can act; the
        // standing-Faded form (6) can recede (§5) — its tile + the base card show the recede mark.
        actionable_tiles: vec![6, 7, 12, 16],
        evolvable_tiles: vec![],
        devolvable_tiles: vec![6],
        actionable_hand,
        evolve_forms: vec![],
        devolve_bases: recede_base.map(|i| vec![i]).unwrap_or_default(),
        lifted_hand: None,
        hand_scroll: 0.0,
        dragging: false,
        drag_xy: None,
        inspect: None,
        replay: None,
        dusk: None,
        result: None,
        choice: None,
    }
}

/// Build a 2v2 board as a full-frame `ShellScene` (the board centred on the page ground), so the
/// preview shows the 6×6 board with the card treatment + palette. Plays a few moves so spirits
/// stand on the board.
fn build_team_board_scene(vw: f32, vh: f32) -> ShellScene {
    use recollect_web::LocalGame;
    use recollect_web::scene::build_team_scene;
    let mut g = LocalGame::new_2v2(0x2222);
    for _ in 0..6 {
        if !place_one(&mut g) {
            // 2v2 auto-advances slots; if no place move, end the turn to rotate.
            let _ = g.end_turn();
        }
    }
    let tv: recollect_core::view::TeamView =
        serde_json::from_str(&g.team_view_json()).expect("team view parses");
    let board_names: Vec<String> = recollect_core::cards::canon_catalog()
        .iter()
        .map(|c| recollect_web::scene::short_board_name(&c.name))
        .collect();
    let board = build_team_scene(&tv, &board_names);
    // Centre the square 6×6 board on the page with a margin.
    let side = (vw.min(vh) * 0.82).floor();
    let bx = (vw - side) / 2.0;
    let by = (vh - side) / 2.0;
    let mut s = ShellScene {
        vw,
        vh,
        board_rect: Rect {
            x: bx,
            y: by,
            w: side,
            h: side,
            color: SColor::rgba(0.949, 0.918, 0.847, 1.0),
            radius: 8.0,
            layer: ShellLayer::Ground,
        },
        board,
        shadows: Vec::new(),
        rects: Vec::new(),
        grads: Vec::new(),
        texts: Vec::new(),
    };
    // A warm page ground + a deep mat surround + a framed board page (mirrors the shell look).
    s.grads.push(GradRect {
        x: 0.0,
        y: 0.0,
        w: vw,
        h: vh,
        top: SColor::rgba(0.196, 0.157, 0.137, 1.0),
        bottom: SColor::rgba(0.137, 0.110, 0.098, 1.0),
        radius: 0.0,
        layer: ShellLayer::Ground,
    });
    // The board page (bright leaf) under the board, with a soft shadow.
    let pad = side * 0.03;
    s.shadows.push(Shadow {
        x: bx - pad,
        y: by - pad,
        w: side + 2.0 * pad,
        h: side + 2.0 * pad,
        radius: 10.0,
        softness: 28.0,
        color: SColor::rgba(0.06, 0.05, 0.08, 0.22),
        layer: ShellLayer::Panel,
    });
    s.rects.push(Rect {
        x: bx - pad,
        y: by - pad,
        w: side + 2.0 * pad,
        h: side + 2.0 * pad,
        color: SColor::rgba(0.988, 0.973, 0.937, 1.0),
        radius: 10.0,
        layer: ShellLayer::Panel,
    });
    s
}

/// Build the FULL canvas shell model for an **online 1v1** telling — the launch-critical path. A
/// real engine plays a few opening moves so the board shows placed cards, then we take its
/// **redacted** `PlayerView` (seat A's vantage — you see only your seat) + the legal-move list and
/// feed `online::shell_model_for_player_view`, exactly as the live client does over the wire (no
/// engine on the client). The still proves the online shell renders the same HUD · hand ·
/// affordances a local telling does, with the opponent as counts/backs only (redaction).
fn build_online_model() -> ShellModel {
    use recollect_core::cards::canon_catalog;
    use recollect_core::types::{CardId, CardKind, Seat};
    use recollect_core::view::view_for;
    use recollect_web::online::shell_model_for_player_view;

    let cat = canon_catalog();
    let deck: Vec<CardId> = cat
        .iter()
        .filter(|c| c.kind == CardKind::Spirit)
        .take(20)
        .map(|c| c.id)
        .collect();
    let (mut e, _) = recollect_core::Engine::new(0x017E, cat, deck.clone(), deck);
    // A few opening plays so the board carries the card treatment (seat A opens).
    for _ in 0..4 {
        let a = e.state().active;
        let play = e.legal_commands(a).into_iter().find(|c| {
            matches!(
                c,
                recollect_core::state::Command::PlaySpirit { engage: None, .. }
            )
        });
        match play {
            Some(cmd) => {
                let _ = e.apply(a, cmd);
            }
            None => break,
        }
    }
    // The redacted view the SERVER would send seat A + the legal list it computes.
    let view = view_for(&e, Seat::A);
    let legal = e.legal_commands(Seat::A);
    shell_model_for_player_view(view, &legal, "Warden Ames", "Corin Ashe")
}

/// Build the FULL canvas shell model for an **online 2v2** telling over the 6×6 `TeamView` — the
/// active slot's vantage (your hand + affordances; the opposing team as combined counts). Built
/// from the engine's redacted `view_for_slot` + the slot's legal list via
/// `online::shell_model_for_team_view`, the exact wire path. The still shows the 6×6 board in the
/// full shell (not the bare board-only `2v2` look), redaction intact.
fn build_online_team_model() -> ShellModel {
    use recollect_core::cards::canon_catalog;
    use recollect_core::types::{CardId, CardKind};
    use recollect_core::view::view_for_slot;
    use recollect_web::online::shell_model_for_team_view;

    let cat = canon_catalog();
    let deck: Vec<CardId> = cat
        .iter()
        .filter(|c| c.kind == CardKind::Spirit)
        .take(20)
        .map(|c| c.id)
        .collect();
    let decks = [deck.clone(), deck.clone(), deck.clone(), deck];
    let (mut e, _) = recollect_core::Engine::new_2v2(0x2222, cat, decks);
    // Play a handful of moves across the rotating slots so the 6×6 board shows placed cards.
    for _ in 0..8 {
        let team = e.state().active_slot.team();
        let play = e.legal_commands(team).into_iter().find(|c| {
            matches!(
                c,
                recollect_core::state::Command::PlaySpirit { engage: None, .. }
            )
        });
        match play {
            Some(cmd) => {
                let _ = e.apply(team, cmd);
            }
            None => {
                let _ = e.apply(team, recollect_core::state::Command::EndTurn);
            }
        }
    }
    let slot = e.state().active_slot;
    let tv = view_for_slot(&e, slot);
    let legal = e.legal_commands(slot.team());
    shell_model_for_team_view(&tv, &legal, "Ally One", "")
}

// ── Build a deterministic ShellModel for each scenario ───────────────────────────────────────
fn build_model(scenario: &str) -> ShellModel {
    use recollect_web::LocalGame;
    let mut g = LocalGame::new(0xC0FFEE);
    // Place a few spirits so the board shows the card treatment; pick the first legal play +
    // first legal tile a couple of times. (Deterministic from the fixed seed.)
    let want_placed = matches!(scenario, "placed" | "inspect" | "lifted");
    if want_placed {
        for _ in 0..4 {
            if !place_one(&mut g) {
                break;
            }
        }
    }
    // The Glimpse choice modals are REAL engine pending choices: drive the live engine
    // to the step, and the shell model carries the prompt (one source with the running client).
    //   • glimpse-burn — open the Glimpse: the burn step (one chip per hand card);
    //   • glimpse-keep — burn one card, then the keep/bottom step (floats the peeked top card).
    if scenario == "glimpse-burn" || scenario == "glimpse-keep" {
        let _ = g.study();
        if scenario == "glimpse-keep" {
            let _ = g.choose(0);
        }
    }
    // The Mulligan offer is the JS opening flag (a legal command, not a pending choice),
    // so pass the same `mulligan_offer` ui the bridge does; the engine's open window honours it.
    let ui = if scenario == "mulligan" {
        r#"{"mulligan_offer":true}"#
    } else {
        "{}"
    };
    let model_json = g.shell_model_json("{}", "{}", ui);
    let mut model: ShellModel = serde_json::from_str(&model_json).expect("shell model parses");
    // For the lifted scenario, lift hand card 0 (if any).
    if scenario == "lifted" && !model.hand.is_empty() {
        model.lifted_hand = Some(0);
    }
    // For the inspect scenario, raise the floating inspect panel for the first hand card,
    // anchored near the tray (the high-frequency inspect moment). The detail is the card's REAL
    // catalog data (stats · reach · keywords · rules) — the same the running client shows — so the
    // still faithfully exercises items 6–10: the coloured stat values, the colon/middot
    // separators, and the lore rendered as a full sentence (item 9 — the render supplies the
    // terminal period the terse game-text omits; no card data is edited).
    if scenario == "inspect"
        && let Some(c) = model.hand.first().cloned()
    {
        use recollect_core::cards::canon_catalog;
        use recollect_web::shell::Inspect;
        let cat = canon_catalog();
        let def = cat.iter().find(|d| d.name == c.name);
        let (reach, keywords, rules, reach_w, reach_center, reach_tiles) = match def {
            Some(d) => {
                let w = 5u8;
                let center = (w / 2) * w + (w / 2);
                let tiles = recollect_core::engine::reach_tiles(
                    d.reach,
                    center,
                    recollect_core::Seat::A,
                    w,
                );
                let mut kw: Vec<String> = Vec::new();
                if d.arcane {
                    kw.push("Arcane".into());
                }
                if d.warded {
                    kw.push("Warded".into());
                }
                if d.mobile {
                    kw.push("Mobile".into());
                }
                if d.steadfast {
                    kw.push("Steadfast".into());
                }
                if d.relentless {
                    kw.push("Relentless".into());
                }
                if d.lurk {
                    kw.push("Lurk".into());
                }
                (
                    format!("{:?}", d.reach),
                    kw,
                    d.rules.clone(),
                    w,
                    center,
                    tiles,
                )
            }
            None => (
                "Cross".into(),
                vec!["Mobile".into(), "Warded".into()],
                "A drifting ember; on arrival, kindle an adjacent ally.".into(),
                5u8,
                12u8,
                vec![7u8, 11, 13, 17],
            ),
        };
        model.inspect = Some(Inspect {
            name: c.name.clone(),
            kind: c.kind.clone(),
            resonance: c.resonance.clone(),
            cost: c.cost,
            attack: c.attack,
            defense: c.defense,
            hp: c.hp,
            reach,
            keywords,
            rules,
            reach_w,
            reach_center,
            reach_tiles,
            anchor: (200.0, 720.0),
        });
    }
    model
}

/// Pick the first legal "place a thing on a tile" move (Play/Cast/Bond/Landmark/Set) and apply
/// it, so the board fills with spirits to show the card treatment. Falls back to the first
/// non-EndTurn move. Uses the labeled list's `cmd` objects + `apply_json`.
fn place_one(g: &mut recollect_web::LocalGame) -> bool {
    let labeled = g.legal_labeled_json();
    let v: serde_json::Value = match serde_json::from_str(&labeled) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return false,
    };
    let is_place = |m: &serde_json::Value| {
        let label = m.get("label").and_then(|l| l.as_str()).unwrap_or("");
        // Labels read like "Play Cinderling to c3" / "Bond …" — anything that lands on a tile.
        label.starts_with("Play")
            || label.starts_with("Bond")
            || label.starts_with("Cast")
            || label.starts_with("Set")
            || label.starts_with("Landmark")
    };
    let pick = arr.iter().find(|m| is_place(m)).or_else(|| {
        arr.iter().find(|m| {
            let l = m.get("label").and_then(|l| l.as_str()).unwrap_or("");
            !l.contains("End Turn")
        })
    });
    if let Some(m) = pick
        && let Some(cmd) = m.get("cmd")
    {
        let _ = g.apply_json(&cmd.to_string());
        return true;
    }
    false
}

// ── CPU rasterizer — a faithful twin of render.rs::draw_shell_scene ───────────────────────────
type Rgba = [f32; 4];

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<Rgba>, // straight (non-premultiplied) RGBA, 0..1
}
impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        // Clear to PAPER (the wgpu pass clears to paper).
        let paper = [0.949, 0.918, 0.847, 1.0];
        Canvas {
            w,
            h,
            px: vec![paper; w * h],
        }
    }
    fn blend(&mut self, x: usize, y: usize, c: Rgba) {
        if x >= self.w || y >= self.h || c[3] <= 0.0 {
            return;
        }
        let i = y * self.w + x;
        let d = self.px[i];
        let a = c[3];
        self.px[i] = [
            c[0] * a + d[0] * (1.0 - a),
            c[1] * a + d[1] * (1.0 - a),
            c[2] * a + d[2] * (1.0 - a),
            1.0,
        ];
    }
}

fn sc(c: SColor) -> Rgba {
    [c.r, c.g, c.b, c.a]
}

/// Signed distance to a rounded box (centre-relative), matching the shader's `sd_round_box`.
fn sd_round_box(px: f32, py: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = px.abs() - bx + r;
    let qy = py.abs() - by + r;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// Fill a rounded-box quad (the SDF fill path): `sdf_half`/`radius`/`softness` mirror the GPU.
#[allow(clippy::too_many_arguments)]
fn fill_sdf(
    cv: &mut Canvas,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    top: Rgba,
    bot: Rgba,
    radius: f32,
    softness: f32,
    sdf_half: Option<(f32, f32)>,
) {
    let (ghw, ghh) = (w * 0.5, h * 0.5);
    let (shw, shh) = sdf_half.unwrap_or((ghw, ghh));
    let r = radius.min(shw.min(shh)).max(0.0);
    let cx = x + ghw;
    let cy = y + ghh;
    let x0 = x.floor().max(0.0) as usize;
    let y0 = y.floor().max(0.0) as usize;
    let x1 = ((x + w).ceil() as usize).min(cv.w);
    let y1 = ((y + h).ceil() as usize).min(cv.h);
    for py in y0..y1 {
        for px in x0..x1 {
            let fx = px as f32 + 0.5 - cx;
            let fy = py as f32 + 0.5 - cy;
            let d = sd_round_box(fx, fy, shw, shh, r);
            let cover = if softness > 0.0 {
                // soft shadow falloff
                1.0 - smoothstep(-softness * 0.35, softness, d)
            } else {
                let aa = 0.75f32;
                1.0 - smoothstep(-aa, aa, d)
            };
            if cover <= 0.0 {
                continue;
            }
            // vertical gradient between top & bot
            let t = ((py as f32 + 0.5 - y) / h).clamp(0.0, 1.0);
            let col = [
                top[0] + (bot[0] - top[0]) * t,
                top[1] + (bot[1] - top[1]) * t,
                top[2] + (bot[2] - top[2]) * t,
                (top[3] + (bot[3] - top[3]) * t) * cover,
            ];
            cv.blend(px, py, col);
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rasterize(scene: &ShellScene, w: usize, h: usize) -> Vec<u8> {
    let mut cv = Canvas::new(w, h);
    let font = FontRef::try_from_slice(FONT_BYTES).expect("font parses");

    // Composite in the same order bands the backend uses.
    let order_of = |layer: ShellLayer| -> u8 {
        match layer {
            ShellLayer::Ground => 0,
            ShellLayer::Panel => 1,
            ShellLayer::Card => 3,
            ShellLayer::Detail => 4,
            ShellLayer::Text => 6,
        }
    };
    const BOARD_ORDER: u8 = 2;
    const TEXT_ORDER: u8 = 6;

    // Gather (order, drawfn) and stable-sort.
    enum Item<'a> {
        Shadow(&'a Shadow),
        Rect(&'a Rect),
        Grad(&'a GradRect),
        BoardQuad(SColor, f32, f32, f32, f32),
        Text(&'a Text),
        BoardLabel(f32, f32, f32, String, SColor),
    }
    let mut items: Vec<(u8, usize, Item)> = Vec::new();
    let mut seq = 0usize;
    for s in &scene.shadows {
        items.push((order_of(s.layer).saturating_sub(1), seq, Item::Shadow(s)));
        seq += 1;
    }
    for r in &scene.rects {
        items.push((order_of(r.layer), seq, Item::Rect(r)));
        seq += 1;
    }
    for g in &scene.grads {
        items.push((order_of(g.layer), seq, Item::Grad(g)));
        seq += 1;
    }
    // Board quads mapped into the board rect.
    let placement = place_board(&scene.board_rect, scene.board.board_w, scene.board.board_h);
    let mut board_quads = scene.board.quads.clone();
    board_quads.sort_by_key(|q| q.layer);
    for q in &board_quads {
        let (x0, y0) = placement.map(q.x, q.y);
        let (x1, y1) = placement.map(q.x + q.w, q.y + q.h);
        items.push((
            BOARD_ORDER,
            seq,
            Item::BoardQuad(q.color, x0, y0, x1 - x0, y1 - y0),
        ));
        seq += 1;
    }
    for label in &scene.board.labels {
        let (lx, ly) = placement.map(label.x, label.y);
        let glyph_px = label.size * placement.sy;
        items.push((
            BOARD_ORDER + 1,
            seq,
            Item::BoardLabel(lx, ly, glyph_px, label.text.clone(), label.color),
        ));
        seq += 1;
    }
    for t in &scene.texts {
        items.push((TEXT_ORDER, seq, Item::Text(t)));
        seq += 1;
    }
    items.sort_by_key(|(o, s, _)| (*o, *s));

    for (_, _, it) in &items {
        match it {
            Item::Shadow(s) => {
                let grow = s.softness * 1.6;
                fill_sdf(
                    &mut cv,
                    s.x - grow,
                    s.y - grow,
                    s.w + 2.0 * grow,
                    s.h + 2.0 * grow,
                    sc(s.color),
                    sc(s.color),
                    s.radius.max(0.0),
                    s.softness,
                    Some((s.w * 0.5, s.h * 0.5)),
                );
            }
            Item::Rect(r) => fill_sdf(
                &mut cv,
                r.x,
                r.y,
                r.w,
                r.h,
                sc(r.color),
                sc(r.color),
                r.radius,
                0.0,
                None,
            ),
            Item::Grad(g) => fill_sdf(
                &mut cv,
                g.x,
                g.y,
                g.w,
                g.h,
                sc(g.top),
                sc(g.bottom),
                g.radius,
                0.0,
                None,
            ),
            Item::BoardQuad(c, x, y, w, h) => {
                // Board quads are plain flat fills (no SDF rounding).
                fill_rect(&mut cv, *x, *y, *w, *h, sc(*c));
            }
            Item::BoardLabel(x, y, px, text, color) => {
                draw_text_centered(&mut cv, &font, *x, *y, *px, text, sc(*color));
            }
            Item::Text(t) => {
                let cx = t.x;
                let w = text_width(&font, &t.text, t.size);
                let anchor_x = match t.align {
                    Align::Left => cx + w / 2.0,
                    Align::Center => cx,
                    Align::Right => cx - w / 2.0,
                };
                // Faux-bold dilation, mirroring render.rs.
                let offsets: &[(f32, f32)] = if t.bold {
                    let d = (t.size * 0.022f32).clamp(0.4, 1.6);
                    &[(0.0, 0.0), (d, 0.0), (0.0, d), (d, d), (-d * 0.5, 0.0)]
                } else {
                    &[(0.0, 0.0)]
                };
                for (ox, oy) in offsets {
                    draw_text_centered(
                        &mut cv,
                        &font,
                        anchor_x + ox,
                        t.y + oy,
                        t.size,
                        &t.text,
                        sc(t.color),
                    );
                }
            }
        }
    }

    // Pack to 8-bit RGBA.
    let mut out = vec![0u8; cv.w * cv.h * 4];
    for (i, p) in cv.px.iter().enumerate() {
        out[i * 4] = (p[0].clamp(0.0, 1.0) * 255.0) as u8;
        out[i * 4 + 1] = (p[1].clamp(0.0, 1.0) * 255.0) as u8;
        out[i * 4 + 2] = (p[2].clamp(0.0, 1.0) * 255.0) as u8;
        out[i * 4 + 3] = 255;
    }
    out
}

fn fill_rect(cv: &mut Canvas, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
    let x0 = x.floor().max(0.0) as usize;
    let y0 = y.floor().max(0.0) as usize;
    let x1 = ((x + w).ceil() as usize).min(cv.w);
    let y1 = ((y + h).ceil() as usize).min(cv.h);
    for py in y0..y1 {
        for px in x0..x1 {
            cv.blend(px, py, c);
        }
    }
}

// ── Text via ab_glyph, centred on (cx, cy) like the atlas's layout_centered ──────────────────
fn text_width(font: &FontRef, text: &str, target_px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(RASTER_PX));
    let s = target_px / RASTER_PX;
    let mut w = 0.0;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        w += scaled.h_advance(gid) * s;
    }
    w
}

fn draw_text_centered(
    cv: &mut Canvas,
    font: &FontRef,
    cx: f32,
    cy: f32,
    target_px: f32,
    text: &str,
    color: Rgba,
) {
    let scaled = font.as_scaled(PxScale::from(RASTER_PX));
    let s = target_px / RASTER_PX;
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let total_w = text_width(font, text, target_px);
    let mut pen_x = cx - total_w / 2.0;
    let line = (ascent - descent) * s;
    let baseline = cy + line / 2.0 + descent * s;
    for ch in text.chars() {
        let gid = font.glyph_id(ch);
        let adv = scaled.h_advance(gid) * s;
        let glyph: Glyph = gid.with_scale(PxScale::from(RASTER_PX));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bb = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let dx = pen_x + (bb.min.x + gx as f32) * s;
                let dy = baseline + (bb.min.y + gy as f32) * s;
                let col = [color[0], color[1], color[2], color[3] * c];
                cv.blend(dx.round() as usize, dy.round() as usize, col);
            });
        }
        pen_x += adv;
    }
}

fn write_png(path: &str, rgba: &[u8], w: usize, h: usize) {
    let file = std::fs::File::create(path).expect("create png");
    let w_ = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w_, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}
