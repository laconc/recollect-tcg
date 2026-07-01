//! The TUI gallery's moments — one source of truth for the `tui_capture` example
//! AND the `tui_gallery` snapshot test. Each moment drives a SEEDED `recollect-core`
//! engine to a point of interest and returns the exact text screen the line-based
//! client prints there: [`render_engine`], the numbered "Legal plays" menu
//! ([`tui_menu_string`]), and [`inspect_card`].
//!
//! It is deterministic in (seed, catalog) — no GPU, no TTY, no stdin — so the
//! committed `docs/gallery/tui/*.txt` goldens are reproducible anywhere and a second
//! regeneration is a no-op. The screens may carry ANSI colour (per `NO_COLOR`); the
//! gallery script sets `NO_COLOR` for stable bytes, and the test compares with colour
//! stripped, so the moment functions stay colour-agnostic.
use crate::render::{inspect_card, render_engine};
use crate::tui_menu_string;
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{decide_opener, generate_deck, offer};
use recollect_core::rng::Rng;
use recollect_core::state::{Command, MatchResult, Phase};
use recollect_core::{Engine, Seat};

/// The fixed capture seed. Chosen so Seat A opens (no initiative bias) and the
/// opening hand + deck are both non-empty — so Glimpse and the once-per-match
/// Mulligan are both legal in round 1. Deterministic: the same screens every run.
pub const SEED: u64 = 6;

/// Every gallery moment, paired with its committed basename under `docs/gallery/tui/`.
/// The example and the snapshot test both iterate this, so the two never drift.
pub const MOMENTS: &[(&str, &str)] = &[
    ("board", "tui-board"),
    ("mulligan", "tui-mulligan"),
    ("glimpse-burn", "tui-glimpse-burn"),
    ("glimpse-keep-bottom", "tui-glimpse-keep-bottom"),
    ("inspect", "tui-inspect"),
    ("result", "tui-result"),
];

/// Render one moment by name to the exact screen string the client prints. Returns
/// `None` for an unknown moment (the example/test report it). See [`MOMENTS`].
pub fn screen(moment: &str) -> Option<String> {
    let s = match moment {
        "board" => board_screen(),
        "mulligan" => mulligan_screen(),
        "glimpse-burn" => glimpse_burn_screen(),
        "glimpse-keep-bottom" => glimpse_keep_bottom_screen(),
        "inspect" => inspect_screen(),
        "result" => result_screen(),
        _ => return None,
    };
    Some(s)
}

/// A fresh, deterministic local engine at the opening — Seat A (Lorekeeper) vs a
/// Lorekeeper Seat B, both on real Quick Play decks (the same `generate_deck` the
/// CLI's `local::run` builds), Seat A opening. Round 1, nobody has acted.
pub fn new_engine() -> Engine {
    let catalog = canon_catalog();
    let style_a = offer(SEED)[0].id;
    let style_b = offer(SEED ^ 0xB)[0].id;
    let deck_a = generate_deck(style_a, SEED, &catalog);
    let deck_b = generate_deck(style_b, SEED.wrapping_add(1), &catalog);
    // No initiative bias ⇒ the seed alone decides the opener; SEED is chosen so A opens.
    let opener = decide_opener(SEED, 0);
    let rules = recollect_core::state::MatchRules::default();
    let (engine, _opening) = Engine::new_with_rules(SEED, catalog, deck_a, deck_b, rules, opener);
    debug_assert_eq!(engine.state().active, Seat::A, "SEED must open Seat A");
    engine
}

/// The opening board the player reads first (no menu) — Seat A's eye view.
fn board_screen() -> String {
    let e = new_engine();
    render_engine(&e, Seat::A)
}

/// The opening DECISION menu — the numbered "Legal plays", which at the opening
/// includes the once-per-match `Mulligan` entry alongside Glimpse / End turn / plays.
fn mulligan_screen() -> String {
    let e = new_engine();
    tui_menu_string(&e, Seat::A)
}

/// The Glimpse (§5) BURN prompt: `Glimpse` opens the burn step, and the menu becomes
/// one `Burn <card> to glimpse` entry per hand card. The full screen (board + menu)
/// is captured, the way the player sees it.
fn glimpse_burn_screen() -> String {
    let mut e = new_engine();
    e.apply(Seat::A, Command::Glimpse)
        .expect("Glimpse is legal at the opening");
    let mut s = render_engine(&e, Seat::A);
    s.push_str(&tui_menu_string(&e, Seat::A));
    s
}

/// The Glimpse keep-or-bottom prompt: after burning the first hand card, the menu
/// becomes exactly `Keep <top> on top` / `Bottom <top> for +1 anima`.
fn glimpse_keep_bottom_screen() -> String {
    let mut e = new_engine();
    e.apply(Seat::A, Command::Glimpse)
        .expect("Glimpse is legal at the opening");
    // Burn the first burnable hand card (Glimpse step 1) — opens the keep/bottom step.
    e.apply(Seat::A, Command::Choose { index: 0 })
        .expect("burning a hand card is legal once Glimpse is open");
    let mut s = render_engine(&e, Seat::A);
    s.push_str(&tui_menu_string(&e, Seat::A));
    s
}

/// The inspect panel for the first hand card — stats, keywords, rules, reach grid —
/// exactly what `i 0` prints. Centred reach (no on-board tile), Seat A's ink.
fn inspect_screen() -> String {
    let e = new_engine();
    let first = e.state().player(Seat::A).hand[0];
    let d = e.card(first).clone();
    inspect_card(&e, &d, None, Seat::A)
}

/// Nightfall: a both-AI playout driven to the result, then the final board plus the
/// `— NIGHTFALL —` line `local::run` prints. Deterministic (seeded bot RNG).
fn result_screen() -> String {
    let mut e = new_engine();
    let mut ai = Rng::from_seed(SEED ^ 0xA1);
    let mut steps = 0;
    while !matches!(e.state().phase, Phase::Finished { .. }) {
        let seat = e.state().active;
        let cmd = recollect_bot::choose(&e, seat, recollect_bot::Difficulty::Normal, &mut ai);
        e.apply(seat, cmd).expect("a bot move is legal");
        steps += 1;
        assert!(steps < 5000, "a both-AI match must terminate");
    }
    let Phase::Finished {
        result,
        score_a,
        score_b,
    } = e.state().phase
    else {
        unreachable!("loop exits only when Finished")
    };
    let mut s = render_engine(&e, e.state().active);
    // The same closing line local::run prints at Nightfall.
    s.push_str(&format!(
        "\n— NIGHTFALL — Score {score_a}–{score_b} · {}\n",
        match result {
            MatchResult::Win(seat) => format!("the match belongs to Seat {seat:?}"),
            MatchResult::Draw => "the Memory keeps both names".to_string(),
        }
    ));
    s
}
