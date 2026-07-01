//! Rendering and command description — the seat's-eye view, in the terminal.
//!
//! Two render paths, one per transport:
//! - [`render_engine`] (local): the rich board the local engine can back —
//!   projections, the legal-move menu's actionable markers, reach, forecast.
//! - [`render_view`] (online): the redacted `PlayerView` is all the wire gives,
//!   so the networked render is necessarily leaner.
//!
//! Both honour redaction: a seat only ever sees its own hand.
//!
//! ## Render-to-string (the screen IS a `String`)
//! [`render_engine`], [`render_view`], and [`inspect_card`] **build and return a
//! `String`** rather than `println!`-ing directly; the callers print what comes
//! back (`print!("{}", render_engine(..))`). Returning a `String` (rather than
//! printing inline) lets the same render feed a **golden text snapshot** (the
//! `tui_capture` example,
//! the `docs/gallery/tui/` goldens) with no TTY, and it is the seam a future
//! cursor-driven TUI draws through. The bodies write through `writeln!`/`write!`
//! into the `String` (which implements [`std::fmt::Write`]); writing to a `String`
//! is infallible, so the `let _ =` on each `writeln!` discards `Ok(())`, not a real error.
use recollect_core::engine::projection;
use recollect_core::state::{Event, Phase, StrikeKind};
use recollect_core::types::{CardDef, tile_xy};
use recollect_core::view::{PlayerView, TeamView, TerrainView};
use recollect_core::{Command, Engine, Seat};
use std::fmt::Write as _;

// --- Color: a small ANSI palette. Disabled if NO_COLOR is set (the standard).
// Seat A is cyan, Seat B is magenta; fading is dim, Echo is yellow, dusk faint.
pub fn color_on() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}
pub fn paint(s: &str, code: &str) -> String {
    if color_on() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}
pub fn seat_color(seat: Seat) -> &'static str {
    match seat {
        Seat::A => "96",
        Seat::B => "95",
    }
} // bright cyan / magenta
pub fn dim(s: &str) -> String {
    paint(s, "2")
}
pub fn bold(s: &str) -> String {
    paint(s, "1")
}
pub fn yellow(s: &str) -> String {
    paint(s, "93")
}

/// An 8-char, owner-tinted board cell for a piece of terrain.
pub fn terrain_cell(terr: &TerrainView) -> String {
    let o = seat_ch(terr.owner);
    let body = match (terr.kind.as_str(), terr.face_down) {
        ("Landmark", _) => format!("  ⌂{o}Lm  "),
        ("Fabrication", true) => format!("  ▒{o}??  "),
        ("Fabrication", false) => format!("  ▒{o}Fb  "),
        _ => format!("  ?{o}?   "),
    };
    paint(&body, seat_color(terr.owner))
}

/// The rich, engine-backed board (local play): the per-seat view plus the
/// projections, actionable markers, dominion tally, and hand that only the
/// in-process engine can compute.
pub fn render_engine(engine: &Engine, seat: Seat) -> String {
    let mut out = String::new();
    let v = recollect_core::view::view_for(engine, seat);
    let proj = projection(engine.state(), seat, engine.catalog_ref());
    let theirs = projection(engine.state(), seat.other(), engine.catalog_ref());
    let st0 = engine.state();
    if st0.round == st0.rules.contraction_after && !st0.contracted {
        let _ = writeln!(
            out,
            "\n  ░░░ DUSK FALLS at this round's end — the empty rim goes dark (marked ░); held spirits keep their light ░░░"
        );
    }
    let _ = writeln!(
        out,
        "\n══ Round {}/{} · Seat {:?} to act · {} ══",
        v.round,
        engine.state().rules.last_round,
        v.active,
        match v.phase {
            Phase::Acting => "your turn".into(),
            Phase::PendingRelease { .. } => "RELEASE a card (hand is full)".into(),
            _ => String::new(),
        }
    );
    for y in (0..5).rev() {
        let mut row = format!("{} ", y + 1);
        for x in 0..5 {
            let t = (y * 5 + x) as usize;
            let tile = &v.tiles[t];
            let dusk_coming = engine.state().round == engine.state().rules.contraction_after
                && !engine.state().contracted
                && recollect_core::types::is_rim(t as u8)
                && tile.spirit.is_none();
            let held = engine.state().contracted
                && recollect_core::types::is_rim(t as u8)
                && !tile.faded
                && tile.spirit.is_some();
            let cell = if tile.faded {
                "░░░░░░░░".into()
            } else if let Some(sp) = &tile.spirit {
                let nm = short(&engine.card(sp.card).name);
                let raw = format!(
                    "{}{}{:>3}{}",
                    seat_ch(sp.owner),
                    nm,
                    sp.hp,
                    if sp.fading && !sp.evolutions.is_empty() {
                        "^"
                    } else if sp.fading {
                        "~"
                    } else if held {
                        "°"
                    } else if sp.echo {
                        "!"
                    } else {
                        " "
                    }
                );
                if sp.fading {
                    dim(&raw)
                } else if sp.echo {
                    yellow(&raw)
                } else {
                    paint(&raw, seat_color(sp.owner))
                }
            } else if let Some(terr) = &tile.terrain {
                terrain_cell(terr)
            } else if let Some(s) = tile.impression {
                paint(&format!("   ·{}   ", seat_ch(s)), seat_color(s))
            } else if proj[t] && theirs[t] {
                "  ::..  ".into()
            } else if proj[t] {
                "   ::   ".into()
            } else if theirs[t] {
                "   ..   ".into()
            } else {
                "        ".into()
            };
            let cell = if dusk_coming && !cell.contains('\x1b') {
                format!("░{}░", &cell[1..cell.len().saturating_sub(1)])
            } else {
                cell
            };
            row.push_str(&format!("[{cell}]"));
        }
        let _ = writeln!(out, "{row}");
    }
    let _ = writeln!(
        out,
        "   a         b         c         d         e      (:: yours · .. theirs · ~ fading · ^ can evolve · ! Echo · ° held/lamplit · ⌂ landmark · ▒ fabrication · ░ dusk)"
    );
    let mut tally = [0u8; 2];
    let legal_now = engine.legal_commands(seat);
    let actionable: std::collections::HashSet<u8> = legal_now
        .iter()
        .filter_map(|c| match c {
            Command::MoveSpirit { from, .. } => Some(*from),
            Command::Evolve { tile, .. } => Some(*tile),
            Command::Reveal { tile, .. } => Some(*tile),
            Command::StrikeFabrication { from, .. } => Some(*from),
            Command::Overwrite { tile, .. } => Some(*tile),
            _ => None,
        })
        .collect();
    // Which of YOUR Mobile spirits can still take their one Move this turn — a
    // tile is "ready to step" iff the engine offers a MoveSpirit from it. A spirit
    // whose tile sits in `moved_this_turn` has spent its move OR just arrived
    // (summoning-sick) and shows as un-movable.
    let can_move_now: std::collections::HashSet<u8> = legal_now
        .iter()
        .filter_map(|c| match c {
            Command::MoveSpirit { from, .. } => Some(*from),
            _ => None,
        })
        .collect();
    let rested = engine.state().moved_this_turn.clone();
    for (i, tile) in engine.state().board.iter().enumerate() {
        if let Some(sp) = &tile.spirit {
            let owner = if sp.fading {
                sp.banished_by.unwrap_or(sp.owner)
            } else {
                sp.owner
            };
            tally[matches!(owner, Seat::B) as usize] += 1;
            let d = engine.card(sp.card);
            let act = if actionable.contains(&(i as u8)) {
                paint("▸", "92")
            } else {
                " ".to_string()
            };
            // A movement cue for YOUR Mobile spirits. ⇢ = ready to step this
            // turn; ⊘ = Mobile but rested (it already moved, or just arrived and is
            // summoning-sick). Opponent spirits and Steadfast ones carry no cue.
            let move_cue = if sp.owner == seat && d.mobile && !sp.fading {
                if can_move_now.contains(&(i as u8)) {
                    paint(" ⇢ can step", "92")
                } else if rested.contains(&(i as u8)) {
                    dim(" ⊘ rested (moved or summoning-sick)")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "  {}{} {} {:<22} Atk {:<3} Def {:<3} HP {:>3}/{:<3} {:?} {:?}{}{}{}",
                act,
                tn(i as u8),
                seat_ch(sp.owner),
                d.name,
                sp.attack,
                sp.defense,
                sp.hp,
                sp.hp_max,
                d.reach,
                d.resonance,
                if sp.fading {
                    " ~fading"
                } else if engine.state().contracted && recollect_core::types::is_rim(i as u8) {
                    " °held (lamplit: scores, no interception, ground goes dark when it leaves)"
                } else {
                    ""
                },
                if sp.echo_eligible() && !sp.fading {
                    " !Echo-live"
                } else {
                    ""
                },
                move_cue,
            );
        } else if let Some(&s) = tile.impressions.first() {
            tally[matches!(s, Seat::B) as usize] += 1;
        }
    }
    let _ = writeln!(
        out,
        "{}",
        dim(
            "(▸ = action available · ⇢ = a Mobile spirit can still step · ⊘ = rested · use the numbered plays below)"
        )
    );
    // The Solace (Seat B) scores its board presence PLUS its off-board erasure
    // tally — every banish or Unwriting it lands counts, and the Unwritten leave no
    // mark on the board, so the tally is the only place that forgetting shows.
    let erasures = engine.state().solace_erasures;
    let b_total = tally[1].saturating_add(erasures);
    let erasure_note = if erasures > 0 {
        format!(" (board {} + {} erased)", tally[1], erasures)
    } else {
        String::new()
    };
    let _ = writeln!(
        out,
        "Score if Nightfall struck now: A {} — B {}{}",
        tally[0], b_total, erasure_note
    );
    // Anima is the real limiter on what you can play — there's no fixed action
    // count, so the turn runs until you End it. Play as much as your Anima funds.
    let _ = writeln!(
        out,
        "You: {} anima (your play budget — no fixed action count; End Turn when ready) · deck {} · hand:",
        v.you.anima, v.you.deck_count
    );
    for (i, c) in v.you.hand.iter().enumerate() {
        let d = engine.card(*c);
        let _ = writeln!(
            out,
            "   {i}. {:<22} {}c {:>2}/{:>2}/{:>2} {:?} {:?}",
            d.name, d.cost, d.attack, d.defense, d.hp, d.reach, d.resonance
        );
    }
    if let Some(p) = v.you.peeked_top {
        let _ = writeln!(
            out,
            "   (Glimpse showed your top page: {})",
            engine.card(p).name
        );
    }
    let _ = writeln!(
        out,
        "Them: {} anima · hand {} · deck {}",
        v.opponent.anima, v.opponent.hand_count, v.opponent.deck_count
    );
    out
}

/// The networked render: all we have over the wire is the redacted `PlayerView`.
pub fn render_view(v: &PlayerView, cat: &[CardDef]) -> String {
    let mut out = String::new();
    let name = |id: recollect_core::CardId| cat[id.0 as usize].name.clone();
    let _ = writeln!(
        out,
        "\n— round {} · {:?} to act · you are {:?} —",
        v.round, v.active, v.seat
    );
    for row in 0..5u8 {
        let mut line = String::new();
        for col in 0..5u8 {
            let i = (row * 5 + col) as usize;
            let t = &v.tiles[i];
            let cell = if let Some(sp) = &t.spirit {
                let tag = if sp.owner == v.seat { 'Y' } else { 'E' };
                // A Mobile spirit of yours that has not yet moved or arrived
                // this turn can still step (⇢); one that has is rested (⊘). Read from
                // the view's `mobile` + `moved_this_turn`, no engine needed online.
                let cue = if sp.owner == v.seat && sp.mobile && !sp.fading {
                    if v.moved_this_turn.contains(&(i as u8)) {
                        '⊘'
                    } else {
                        '⇢'
                    }
                } else {
                    ' '
                };
                format!("{tag}:{:<9.9}{cue}{:>3}", name(sp.card), sp.hp)
            } else if t.faded {
                "░░ faded ░░    ".into()
            } else if let Some(s) = t.impression {
                format!("impression {s:?}       ")
            } else {
                String::from("·              ")
            };
            line.push_str(&format!("[{i:>2} {cell}]"));
        }
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(
        out,
        "anima {} · deck {} · opp hand {} deck {} anima {}",
        v.you.anima,
        v.you.deck_count,
        v.opponent.hand_count,
        v.opponent.deck_count,
        v.opponent.anima
    );
    // The Solace scores its off-board erasure tally too — show it mid-game so
    // the standing is legible before Nightfall folds it in (the Unwritten leave no
    // board mark). Surfaced from the view, so it works on the online path now.
    if v.solace_erasures > 0 {
        let _ = writeln!(
            out,
            "Solace erasures (off-board score): {} (⇢ a Mobile spirit can still step · ⊘ rested)",
            v.solace_erasures
        );
    }
    for (i, id) in v.you.hand.iter().enumerate() {
        let c = &cat[id.0 as usize];
        let _ = writeln!(
            out,
            "  hand {i}: {} (cost {} · {}/{}/{} · {:?} · {:?})",
            c.name, c.cost, c.attack, c.defense, c.hp, c.reach, c.resonance
        );
    }
    match &v.phase {
        Phase::Finished {
            result,
            score_a,
            score_b,
        } => {
            let _ = writeln!(out, "THE MATCH ENDS — {result:?} ({score_a}–{score_b})");
        }
        Phase::PendingRelease { seat, .. } if *seat == v.seat => {
            let _ = writeln!(out, "hand cap: you must release (r <hand#>)");
        }
        _ => {}
    }
    out
}

/// The networked 2v2 render — a slot's `TeamView` on the 6×6 board. Like
/// [`render_view`] but team-shaped: `Y` = your team, `E` = a rival, plus your
/// hand and the public counts for your teammate and the two opponents.
pub fn render_team(v: &TeamView, cat: &[CardDef]) {
    let name = |id: recollect_core::CardId| cat[id.0 as usize].name.clone();
    let w = v.board_w.max(1) as usize;
    println!(
        "\n— 2v2 · round {} · slot {:?} to act — you are {:?} (team {:?}) —",
        v.round, v.active_slot, v.slot, v.team
    );
    for row in 0..w {
        let mut line = String::new();
        for col in 0..w {
            let i = row * w + col;
            let t = &v.tiles[i];
            let cell = if let Some(sp) = &t.spirit {
                let tag = if sp.owner == v.team { 'Y' } else { 'E' };
                format!("{tag}:{:<7.7}{:>3}", name(sp.card), sp.hp)
            } else if t.faded {
                "░ faded ░ ".into()
            } else if let Some(s) = t.impression {
                format!("impression {s:?}  ")
            } else {
                String::from("·         ")
            };
            line.push_str(&format!("[{i:>2} {cell}]"));
        }
        println!("{line}");
    }
    println!(
        "you (slot {:?}): {} anima · deck {} · hand:",
        v.slot, v.you.anima, v.you.deck_count
    );
    for (i, id) in v.you.hand.iter().enumerate() {
        let c = &cat[id.0 as usize];
        println!(
            "  hand {i}: {} (cost {} · {}/{}/{} · {:?} · {:?})",
            c.name, c.cost, c.attack, c.defense, c.hp, c.reach, c.resonance
        );
    }
    println!(
        "teammate: hand {} · deck {} · anima {}",
        v.teammate.hand_count, v.teammate.deck_count, v.teammate.anima
    );
    for (k, o) in v.opponents.iter().enumerate() {
        println!(
            "opponent {}: hand {} · deck {} · anima {}",
            k + 1,
            o.hand_count,
            o.deck_count,
            o.anima
        );
    }
    // The Solace team's off-board erasure tally, mid-game (see `render_view`).
    if v.solace_erasures > 0 {
        println!("Solace erasures (off-board score): {}", v.solace_erasures);
    }
    if let Phase::Finished {
        result,
        score_a,
        score_b,
    } = &v.phase
    {
        println!("THE MATCH ENDS — {result:?} ({score_a}–{score_b})");
    }
}

/// Show everything about a card: stats, keywords, effect text, and a little grid
/// of the tiles its reach threatens (★ = the card, ● = a tile it can engage).
pub fn inspect_card(engine: &Engine, d: &CardDef, at_tile: Option<u8>, owner: Seat) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "\n  ┌─ {} ─{}",
        bold(&d.name),
        "─".repeat(28usize.saturating_sub(d.name.len()))
    );
    let _ = writeln!(
        out,
        "  │ {:?} · {:?} · cost {}",
        d.kind, d.resonance, d.cost
    );
    let _ = writeln!(out, "  │ Atk {}  Def {}  HP {}", d.attack, d.defense, d.hp);
    let mut kw = Vec::new();
    if d.arcane {
        kw.push("Arcane");
    }
    if d.warded {
        kw.push("Warded");
    }
    if d.mobile {
        kw.push("Mobile");
    }
    if d.steadfast {
        kw.push("Steadfast");
    }
    if d.relentless {
        kw.push("Relentless");
    }
    if d.lurk {
        kw.push("Lurk");
    }
    if !kw.is_empty() {
        let _ = writeln!(out, "  │ {}", kw.join(" · "));
    }
    if !d.rules.trim().is_empty() {
        let _ = writeln!(out, "  │ {}", d.rules.trim());
    }
    let w = engine.state().board_w as u8;
    let tile = at_tile.unwrap_or(w * w / 2);
    let reach: Vec<u8> = recollect_core::engine::reach_tiles(d.reach, tile, owner, w);
    let _ = writeln!(out, "  │ Reach {:?}:", d.reach);
    for y in (0..w).rev() {
        let mut row = String::from("  │   ");
        for x in 0..w {
            let t = y * w + x;
            let g = if t == tile {
                paint("★", "1")
            } else if reach.contains(&t) {
                paint("●", seat_color(owner))
            } else {
                dim("·")
            };
            row.push_str(&format!("{g} "));
        }
        let _ = writeln!(out, "{row}");
    }
    let _ = writeln!(out, "  └{}", "─".repeat(34));
    out
}

/// A one-line gloss of a legal command, with combat forecast for engagements.
/// Delegates to the canonical labeler in `recollect-protocol` so the local TUI,
/// the server's wire `LegalMove`s, and any future client all read identically.
pub fn describe(engine: &Engine, seat: Seat, cmd: &Command) -> String {
    recollect_protocol::label(engine, seat, cmd)
}

pub fn narrate(engine: &Engine, events: &[Event]) {
    let before = engine.state().clone();
    narrate_with(engine, &before, events)
}

pub fn narrate_with(engine: &Engine, before: &recollect_core::GameState, events: &[Event]) {
    use recollect_core::AggregateRules;
    let mut cur = before.clone();
    for ev in events {
        let line = match ev {
            Event::SpiritPlayed {
                seat, card, tile, ..
            } => Some(format!(
                "{} arrives at {} for Seat {seat:?}.",
                engine.card(*card).name,
                tn(*tile)
            )),
            Event::Struck {
                from_tile,
                to_tile,
                damage,
                echo,
                kind,
            } => Some(format!(
                "{} strikes {} for {}{}{}.",
                name_at(engine, &cur, *from_tile),
                name_at(engine, &cur, *to_tile),
                damage,
                if *echo { " (ECHO +20)" } else { "" },
                match kind {
                    StrikeKind::Interception => " — interception",
                    StrikeKind::Retaliation => " in answer",
                    StrikeKind::Chain(n) if *n >= 1 => " — momentum",
                    _ => "",
                }
            )),
            Event::SpiritBecameFading { tile, .. } => Some(format!(
                "{} is banished from the match.",
                name_at(engine, &cur, *tile)
            )),
            Event::Overwrote {
                seat,
                card,
                tile,
                success,
                ..
            } => Some(if *success {
                format!(
                    "{} OVERWRITES {} — the banisher's impression at {}.",
                    engine.card(*card).name,
                    tn(*tile),
                    tn(*tile)
                )
            } else {
                format!(
                    "Seat {seat:?}'s overwrite breaks on {} and dissolves — the wound stays.",
                    name_at(engine, before, *tile)
                )
            }),
            Event::SpiritDissolved { tile, impression } => Some(format!(
                "the spirit at {} dissolves; the impression is Seat {impression:?}'s.",
                tn(*tile)
            )),
            // Overwrite reaches a Stray (§2): a REVEALED Stray is a fought exchange —
            // success ⇒ the overwriter banishes it and takes the cleared tile (the
            // banisher's impression beneath); failure ⇒ the overwriter dissolves, the
            // wound persisting on the wild. (`damage_to_stray == 0 && success` is the
            // uncontested arrival after a hidden Stray was denied — narrated by
            // `StrayDenied`, so the gloss stays plain here.)
            Event::OverwroteStray {
                card,
                tile,
                success,
                ..
            } => Some(if *success {
                format!(
                    "{} OVERWRITES the Stray at {} — the banisher's impression beneath.",
                    engine.card(*card).name,
                    tn(*tile)
                )
            } else {
                format!(
                    "{}'s overwrite breaks on the Stray at {} and dissolves — the wound stays.",
                    engine.card(*card).name,
                    tn(*tile)
                )
            }),
            // A HIDDEN Stray is denied entry, not fought (§2): it leaves with no
            // impression, no reveal, nothing — and is NEVER named (the veil's redaction
            // holds; the event carries no CardId by design). The overwriter then takes
            // the cleared tile as an uncontested arrival.
            Event::StrayDenied { tile } => Some(format!(
                "the overwrite at {} finds only a veil — the hidden thing is denied entry and is gone, leaving no mark.",
                tn(*tile)
            )),
            // Devolution (§5) — the rescue: a banished form recedes a tier to a held base
            // (full HP, fade cleared). In-register by faction: the Lorekeeper REVERTS, the
            // Solace RECEDES (one engine action, the faction's verb in text).
            Event::SpiritDevolved { seat, tile, to, .. } => {
                let verb = match engine.state().rules.factions[*seat as usize] {
                    recollect_core::types::Faction::Solace => "recedes",
                    recollect_core::types::Faction::Lorekeeper => "reverts",
                };
                Some(format!(
                    "Seat {seat:?} {verb} the faded form at {} to {} — rescued, one tier down.",
                    tn(*tile),
                    engine.card(*to).name
                ))
            }
            Event::MemoryContracted { .. } => Some(
                "DUSK FALLS — the empty rim goes dark; impressions lock; held spirits keep their light."
                    .into(),
            ),
            Event::RoundAdvanced { round } => Some(format!(
                "— round {round}{} —",
                if *round == 11 {
                    " · THE ELEVENTH HOUR"
                } else if *round == 12 {
                    " · Nightfall approaches"
                } else {
                    ""
                }
            )),
            // Throughline completes (§5.4): a connected 3-line of one Imprint forms and
            // the completing spirit gains +A/+D and a full heal (once per body — fading,
            // Primal-evolve, and devolution reset it; Fabled keeps it). A real beat worth
            // calling.
            Event::ThroughlineCompleted {
                tile,
                attack,
                defense,
            } => Some(format!(
                "{} completes a Throughline — +{attack}/+{defense} and a full restore.",
                name_at(engine, &cur, *tile)
            )),
            Event::Glimpsed { seat, .. } => Some(format!("Seat {seat:?} glimpses the page.")),
            // Glimpse (§5) BURN cost — narrate the beat, not the card (which card the
            // glimpser spent is private; the opponent learns only THAT one was burned).
            Event::GlimpseBurned { seat, .. } => {
                Some(format!("Seat {seat:?} burns a card from hand to glimpse."))
            }
            // Glimpse (§5) settled — narrate the verdict, not the card (the card the
            // glimpser saw is private; the opponent learns only the keep-or-bottom beat).
            Event::GlimpseResolved { seat, kept } => Some(if *kept {
                format!("Seat {seat:?} keeps the top card.")
            } else {
                format!("Seat {seat:?} bottoms it for +1 anima.")
            }),
            _ => None,
        };
        if let Some(l) = line {
            println!("  {l}");
        }
        cur.evolve(ev);
    }
}

fn name_at(engine: &Engine, cur: &recollect_core::GameState, t: u8) -> String {
    match cur.spirit_at(t) {
        Some(s) => format!("{} ({})", engine.card(s.card).name, tn(t)),
        None => tn(t),
    }
}

pub fn tn(t: u8) -> String {
    let (x, y) = tile_xy(t);
    format!("{}{}", (b'a' + x as u8) as char, y + 1)
}
pub fn seat_ch(s: Seat) -> char {
    match s {
        Seat::A => 'A',
        Seat::B => 'B',
    }
}
fn short(name: &str) -> String {
    let s: String = name.chars().filter(|c| c.is_alphabetic()).take(4).collect();
    format!("{s:<4}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::types::CardId;
    use recollect_core::view::TerrainView;

    fn terr(kind: &str, face_down: bool) -> TerrainView {
        TerrainView {
            card: CardId(0),
            owner: Seat::A,
            kind: kind.into(),
            face_down,
        }
    }

    #[test]
    fn terrain_cells_render_their_kind_glyph() {
        assert!(
            terrain_cell(&terr("Landmark", false)).contains("Lm"),
            "landmark tag"
        );
        assert!(
            terrain_cell(&terr("Fabrication", true)).contains("??"),
            "a face-down Fabrication shows as a veiled lie"
        );
        assert!(
            terrain_cell(&terr("Fabrication", false)).contains("Fb"),
            "a revealed Fabrication shows its tag"
        );
    }
}
