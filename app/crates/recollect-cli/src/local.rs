//! Local transport: the engine runs in-process. The same `recollect-core` the
//! server embeds, so a hotseat/vs-AI match is rule-identical to a networked one
//! — only nothing crosses a socket. Drives the rich [`render_engine`] TUI.
use crate::render::{bold, describe, dim, inspect_card, narrate, narrate_with, render_engine, tn};
use crate::verbs;
use recollect_bot::Difficulty;
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{
    LOREKEEPER_CHARACTERS, SOLACE_CHARACTERS, generate_deck, lorekeeper_character_deck, offer,
    preview, solace_character_deck,
};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchRules, Phase};
use recollect_core::types::{CardId, Faction};
use recollect_core::{Command, Engine, Seat};
use std::io::{self, BufRead, IsTerminal, Write};

/// Wall-clock seed (the CLI is a client; the ENGINE never sees a clock).
pub fn now_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(42)
}

/// One local match. `a_is_ai`/`b_is_ai` choose who the engine plays; `difficulty`
/// is the AI tier (ignored for purely-human seats). `opp_faction` is the faction
/// the **bot** seat (B) fields — when B is AI it fields a NAMED character of that
/// faction (character parity), picked + deck-derived exactly like the server's
/// matchmaking; a human seat B ignores it (it picks its own style).
pub fn run(seed: u64, a_is_ai: bool, b_is_ai: bool, difficulty: Difficulty, opp_faction: Faction) {
    let catalog = canon_catalog();
    println!("RECOLLECT — quick play · seed {seed} (keep it to replay this exact match)");
    // Seat A is the human (or a spectated AI in `watch`); always a Lorekeeper, picking a style.
    let style_a = pick_style("Seat A", seed, a_is_ai);
    let deck_a = generate_deck(style_a, seed, &catalog);
    // Seat B: a bot opponent fields a NAMED character of the chosen faction (parity with online —
    // `matchmaking.rs` picks `seed % roster` and salts the deck by `seed+1`); a human seat B picks
    // a Quick Play style. The character drives B's deck, B's faction, and an initiative bias on the
    // opener toss — the same three levers the server wires.
    let (deck_b, faction_b, bias) = if b_is_ai {
        let opp = pick_bot_character(opp_faction, seed, &catalog);
        announce_opponent(&opp);
        // The character's initiative leans the seeded opener toward ITS seat (B), so the bias is
        // negated (decide_opener's positive bias favours A). An edge, never a guarantee.
        (opp.deck, opp.faction, -opp.initiative)
    } else {
        let style_b = pick_style("Seat B", seed ^ 0xB, false);
        (
            generate_deck(style_b, seed.wrapping_add(1), &catalog),
            Faction::Lorekeeper,
            0,
        )
    };
    // First-player: the opener is a seeded flip (parity with the server) weighted by the bot
    // character's initiative, not hardcoded A. The loop below drives whichever seat opens —
    // including a bot opener — so nothing is stranded.
    let opener = recollect_core::quickplay::decide_opener(seed, bias);
    // The faction is per-seat: a Solace seat B lets the engine tally its off-board erasures (its
    // removals leave no impression), exactly as the server's `new_with_opener` does.
    let rules = MatchRules {
        factions: [Faction::Lorekeeper, faction_b],
        ..MatchRules::default()
    };
    let (mut engine, opening) =
        Engine::new_with_rules(seed, catalog, deck_a, deck_b, rules, opener);
    let mut ai = Rng::from_seed(seed ^ 0xA1);
    narrate(&engine, &opening);
    // Cursor TUI: a real terminal + a local 1v1 vs the AI (Seat A human, Seat B the
    // bot) gets the ratatui arrow-key cursor — true parity with the web's gold cursor.
    // Everything else keeps the line REPL below, byte-for-byte: `hotseat` (two humans) and
    // `watch` (two AIs), and any non-TTY run (CI, `--json`/headless, a pipe — where
    // `IsTerminal` is false). The cursor loop drives the whole match (human + AI turns) and
    // returns at Nightfall or on `q`; the existing finish-print path below then closes it,
    // so Nightfall reads identically across modes.
    let cursor_eligible = !a_is_ai && b_is_ai && io::stdout().is_terminal();
    if cursor_eligible {
        match crate::tui::run_local(&mut engine, Seat::A, Seat::B, difficulty, &mut ai) {
            Ok(()) => {}
            Err(e) => eprintln!("(cursor TUI unavailable: {e}; falling back to the line view)"),
        }
        // Whether the match finished or the player quit, print the closing state on the
        // normal screen (the same lines the line loop prints), then we're done.
        if let Phase::Finished {
            result,
            score_a,
            score_b,
        } = engine.state().phase
        {
            print!("{}", render_engine(&engine, engine.state().active));
            println!(
                "\n— NIGHTFALL — Score {score_a}–{score_b} · {}",
                match result {
                    recollect_core::MatchResult::Win(s) =>
                        format!("the match belongs to Seat {s:?}"),
                    recollect_core::MatchResult::Draw => "the Memory keeps both names".into(),
                }
            );
        } else {
            println!("\n(the match pauses here — seed {seed} resumes it)");
        }
        return;
    }
    loop {
        if let Phase::Finished {
            result,
            score_a,
            score_b,
        } = engine.state().phase
        {
            print!("{}", render_engine(&engine, engine.state().active));
            println!(
                "\n— NIGHTFALL — Score {score_a}–{score_b} · {}",
                match result {
                    recollect_core::MatchResult::Win(s) =>
                        format!("the match belongs to Seat {s:?}"),
                    recollect_core::MatchResult::Draw => "the Memory keeps both names".into(),
                }
            );
            return;
        }
        let seat = engine.state().active;
        let is_ai = matches!(seat, Seat::A) && a_is_ai || matches!(seat, Seat::B) && b_is_ai;
        let cmd = if is_ai {
            recollect_bot::choose(&engine, seat, difficulty, &mut ai)
        } else {
            print!("{}", render_engine(&engine, seat));
            prompt_command(&engine, seat)
        };
        if is_ai {
            println!(
                "\n· Seat {seat:?} ({} bot) — {}",
                difficulty.name(),
                describe(&engine, seat, &cmd)
            );
        }
        let before = engine.state().clone();
        match engine.apply(seat, cmd) {
            Ok(events) => narrate_with(&engine, &before, &events),
            Err(r) => println!("  the Memory declines: {r:?}"),
        }
    }
}

/// The named character a bot seat fields this match: its identity (name + lore) for the
/// who-you-face announce, the faction-pure deck it pilots, and its first-player initiative.
/// Built purely from (faction, seed) so the match stays re-derivable.
struct OpponentCharacter {
    name: &'static str,
    lore: &'static str,
    faction: Faction,
    deck: Vec<CardId>,
    /// First-player initiative — how hard this character leans toward opening (≥0). Fed as a
    /// bias to `decide_opener`, signed by the caller for the bot's seat.
    initiative: i32,
}

/// Pick the bot opponent's named character + derive its deck, the SAME way the server's
/// matchmaking does (`recollect-server/src/matchmaking.rs`): index the faction roster by
/// `seed % len`, then build the faction deck salted by `seed + 1`. Pure in (faction, seed,
/// catalog) — deterministic, so the same seed always faces the same character with the same
/// deck (the determinism covenant). The Solace draws by **disposition**, the Lorekeeper by
/// **style**; both are real, legal faction-pure 20-card decks the bot pilots through normal play.
fn pick_bot_character(
    faction: Faction,
    seed: u64,
    catalog: &[recollect_core::types::CardDef],
) -> OpponentCharacter {
    match faction {
        Faction::Solace => {
            let i = (seed % SOLACE_CHARACTERS.len() as u64) as usize;
            let ch = &SOLACE_CHARACTERS[i];
            OpponentCharacter {
                name: ch.name,
                lore: ch.lore,
                faction,
                deck: solace_character_deck(i, seed.wrapping_add(1), catalog),
                initiative: ch.initiative(),
            }
        }
        Faction::Lorekeeper => {
            let i = (seed % LOREKEEPER_CHARACTERS.len() as u64) as usize;
            let ch = &LOREKEEPER_CHARACTERS[i];
            OpponentCharacter {
                name: ch.name,
                lore: ch.lore,
                faction,
                deck: lorekeeper_character_deck(i, seed.wrapping_add(1), catalog),
                initiative: ch.initiative(),
            }
        }
    }
}

/// Announce who the player faces — name + faction + lore — the local parity of the online
/// who-opens / opponent-identity flow. Printed before the opening narrate so the player
/// meets their opponent the way the web client names them in its live region.
fn announce_opponent(opp: &OpponentCharacter) {
    println!("\n{}", opponent_intro(opp.name, opp.faction));
    println!("  {}", dim(opp.lore));
}

/// The one-line opponent intro, pure for testing: `You face {name}, {role}.`, where the role
/// is a Solace tempter or a Lorekeeper keeper. The faction phrase keeps the game's register (the
/// Solace tempts you to forget; a Lorekeeper keeps the Memory).
fn opponent_intro(name: &str, faction: Faction) -> String {
    let role = match faction {
        Faction::Solace => "a tempter of the Solace",
        Faction::Lorekeeper => "a Lorekeeper, a keeper of the Memory",
    };
    format!("You face {}, {role}.", bold(name))
}

fn pick_style(seat: &str, seed: u64, auto: bool) -> u8 {
    let offers = offer(seed);
    if auto {
        return offers[0].id;
    }
    let cat = canon_catalog();
    println!("\n{seat} — the Memory offers three plays:");
    for (i, s) in offers.iter().enumerate() {
        println!("  {}. {:<18} {}", i + 1, bold(s.name), s.blurb);
        // The OBJECTIVE shape, beside the voice: resonance lean · aggression · tempo ·
        // body-mix, computed in core over many deck-gen seeds — so the pick is informed,
        // not just a feel. The same `summary()` the web picker reads.
        println!("     {}", s.selection.summary());
        let pv = preview(s.id, seed, &cat);
        let curve: String = pv
            .curve
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(c, n)| format!("{c}:{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "     {} spirits · {} spellbook · cost curve {}",
            pv.spirit_count, pv.spell_count, curve
        );
        let opens: Vec<&str> = pv.cards.iter().take(5).map(|c| c.name.as_str()).collect();
        println!("     {}", dim(&format!("opens: {}", opens.join(", "))));
    }
    loop {
        if let Some(n) = read_number("choose a style", 1, 3) {
            return offers[n - 1].id;
        }
    }
}

/// The numbered "Legal plays" menu as a `String` — every legal command for
/// `seat`, each on its own line through the canonical [`describe`] labeler, plus
/// the one-line input hint. Built (not printed) so [`prompt_command`] can emit it
/// AND the `tui_capture` example can snapshot the same menu the player reads (the
/// Glimpse burn / keep-bottom prompts and the opening Mulligan entry all surface
/// here, since they're just `legal_commands` the engine offers). Pure (no I/O).
pub(crate) fn menu_string(engine: &Engine, seat: Seat) -> String {
    use std::fmt::Write as _;
    let legal = engine.legal_commands(seat);
    let mut out = String::new();
    let _ = writeln!(out, "\nLegal plays:");
    for (i, c) in legal.iter().enumerate() {
        let _ = writeln!(out, "  {i:>3}. {}", describe(engine, seat, c));
    }
    let _ = writeln!(
        out,
        "  {}",
        dim(
            "(type a number to act · or a verb: p/o/m/v/dv/rc/g/r/end · 'i N' inspect hand card N · 'i <tile>' e.g. 'i c3' inspect a board card)"
        )
    );
    out
}

fn prompt_command(engine: &Engine, seat: Seat) -> Command {
    let legal = engine.legal_commands(seat);
    print!("{}", menu_string(engine, seat));
    let w = engine.state().board_w as u8;
    loop {
        print!("your move > ");
        io::stdout().flush().ok();
        let mut line = String::new();
        let bytes = io::stdin().lock().read_line(&mut line).unwrap_or(0);
        let t = line.trim();
        if bytes == 0 || t == "q" {
            println!("\n(the match pauses here — seed printed above resumes it)");
            std::process::exit(0);
        }
        if let Some(arg) = t.strip_prefix("i ").map(|s| s.trim()) {
            if let Some(tile) = verbs::parse_tile(arg, w) {
                if let Some(sp) = engine.state().spirit_at(tile) {
                    let d = engine.card(sp.card).clone();
                    print!("{}", inspect_card(engine, &d, Some(tile), sp.owner));
                } else if let Some(terr) = engine.state().board[tile as usize].terrain.as_ref() {
                    let d = engine.card(terr.card).clone();
                    print!("{}", inspect_card(engine, &d, Some(tile), terr.owner));
                } else {
                    println!("  (nothing on {})", tn(tile));
                }
            } else if let Ok(hi) = arg.parse::<usize>() {
                if let Some(c) = engine.state().player(seat).hand.get(hi) {
                    let d = engine.card(*c).clone();
                    print!("{}", inspect_card(engine, &d, None, seat));
                } else {
                    println!("  (no hand card {hi})");
                }
            } else {
                println!("  (inspect: 'i 0'..'i N' for hand, or 'i c3' for a board tile)");
            }
            continue;
        }
        // Everything else is a move: a bare number from the menu, OR a verb.
        match move_for_line(t, &legal, w) {
            Ok(cmd) => return cmd,
            Err(hint) => println!("  ({hint})"),
        }
    }
}

/// Turn one input line into a move, or an error hint. A bare number picks from the
/// numbered `legal` menu; anything else is parsed as the shared verb grammar
/// — the SAME `verbs` parser the online TUI uses, so local and online speak one
/// language. `Evolve`/`Reclaim` verbs name only a tile and resolve against `legal`;
/// a fully-specified verb (Play/Move/…) passes straight through to the engine,
/// which validates it. Pure (no I/O) so the dispatch is unit-testable.
fn move_for_line(line: &str, legal: &[Command], w: u8) -> Result<Command, String> {
    if let Ok(n) = line.parse::<usize>() {
        return legal.get(n).cloned().ok_or_else(|| {
            format!(
                "no move {n} — enter 0-{}, a verb, 'i N' to inspect, or 'q'",
                legal.len().saturating_sub(1)
            )
        });
    }
    match verbs::parse(line, w) {
        Some(intent) => verbs::resolve(intent, legal)
            .ok_or_else(|| "no legal Evolve/Reclaim on that tile — read the numbered plays".into()),
        None => Err(verbs::USAGE.into()),
    }
}

fn read_number(what: &str, lo: usize, hi: usize) -> Option<usize> {
    print!("{what} [{lo}-{hi}] > ");
    io::stdout().flush().ok();
    let mut line = String::new();
    let bytes = io::stdin().lock().read_line(&mut line).ok()?;
    if bytes == 0 || line.trim() == "q" {
        println!("\n(the match pauses here — seed printed above resumes it)");
        std::process::exit(0);
    }
    let n: usize = line.trim().parse().ok()?;
    (lo..=hi).contains(&n).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A small legal set to dispatch against.
    fn legal() -> Vec<Command> {
        vec![
            Command::Glimpse, // 0
            Command::EndTurn, // 1
            Command::MoveSpirit {
                from: 12,
                to: 13,
                engage: None,
            }, // 2
            Command::Evolve {
                tile: 7,
                form_hand: 0,
                fuel: None,
                engage: None,
            }, // 3
            Command::Reclaim { tile: 18 }, // 4
            Command::Devolve {
                tile: 9,
                base_hand: 1,
            }, // 5
        ]
    }

    #[test]
    fn local_dispatch_takes_numbered_menu_picks() {
        let l = legal();
        assert_eq!(move_for_line("0", &l, 5), Ok(Command::Glimpse));
        assert_eq!(move_for_line("1", &l, 5), Ok(Command::EndTurn));
        // Out of range ⇒ a hint, not a pick.
        assert!(move_for_line("99", &l, 5).is_err());
    }

    #[test]
    fn local_now_parses_the_shared_verbs() {
        // The verb grammar local play uses, through the one shared `verbs` parser
        // (the same parser the online TUI uses).
        let l = legal();
        // Glimpse / End turn by verb.
        assert_eq!(move_for_line("g", &l, 5), Ok(Command::Glimpse));
        assert_eq!(move_for_line("end", &l, 5), Ok(Command::EndTurn));
        // A fully-specified Play passes straight through (grid coords accepted).
        assert_eq!(
            move_for_line("p 0 c3", &l, 5),
            Ok(Command::PlaySpirit {
                hand_index: 0,
                tile: 12,
                engage: None,
                chain_prefs: Vec::new(),
            })
        );
        // Move with both grid coords and an engage clause.
        assert_eq!(
            move_for_line("m c3 d3 e c4", &l, 5),
            Ok(Command::MoveSpirit {
                from: 12,
                to: 13,
                engage: Some(17),
            })
        );
    }

    #[test]
    fn local_evolve_and_reclaim_verbs_resolve_against_the_legal_set() {
        // The Evolve (a held form card played onto its base) and the Reclaim are
        // tile-named verbs: `v <tile>` / `rc <tile>` resolve to the concrete
        // legal command on that tile.
        let l = legal();
        assert_eq!(
            move_for_line("v 7", &l, 5),
            Ok(Command::Evolve {
                tile: 7,
                form_hand: 0,
                fuel: None,
                engage: None,
            })
        );
        // An off-board tile token (col g is index 6 on a 5-wide board) ⇒ usage hint.
        assert_eq!(move_for_line("evolve g1", &l, 5), Err(verbs::USAGE.into()));
        assert_eq!(
            move_for_line("rc 18", &l, 5),
            Ok(Command::Reclaim { tile: 18 })
        );
        // A tile with no legal Evolve ⇒ a clear hint, not a bogus command.
        assert!(move_for_line("v 3", &l, 5).is_err());
    }

    #[test]
    fn local_devolve_verb_resolves_against_the_legal_set() {
        // Devolution (§5) the rescue — `dv <tile>` / `devolve <tile>` names the
        // standing-Faded form tile and resolves to the legal `Devolve` on it.
        let l = legal();
        assert_eq!(
            move_for_line("dv 9", &l, 5),
            Ok(Command::Devolve {
                tile: 9,
                base_hand: 1,
            })
        );
        assert_eq!(
            move_for_line("devolve 9", &l, 5),
            Ok(Command::Devolve {
                tile: 9,
                base_hand: 1,
            })
        );
        // A tile with no legal Devolve ⇒ a clear hint, not a bogus command.
        assert!(move_for_line("dv 3", &l, 5).is_err());
    }

    #[test]
    fn local_rejects_gibberish_with_the_usage_hint() {
        let l = legal();
        assert_eq!(move_for_line("dance", &l, 5), Err(verbs::USAGE.into()));
    }

    /// §5 Mulligan — LOCAL mode speaks it too (not just online). The opening offers a
    /// `Mulligan { seat }` in the legal menu; the `mull` (and `mulligan`) verb resolves
    /// against that legal set to the concrete seat-stamped command, exactly as the
    /// numbered entry would — through the SAME shared `verbs` parser. (Glimpse + Mulligan
    /// are fully wired across local/headless/online; this guards the local verb path.)
    #[test]
    fn local_mulligan_verb_resolves_against_the_opening_legal_set() {
        // The opening legal set: the Mulligan offer alongside the always-present moves.
        let l = vec![
            Command::Glimpse,
            Command::EndTurn,
            Command::Mulligan { seat: Seat::A },
        ];
        assert_eq!(
            move_for_line("mull", &l, 5),
            Ok(Command::Mulligan { seat: Seat::A })
        );
        assert_eq!(
            move_for_line("mulligan", &l, 5),
            Ok(Command::Mulligan { seat: Seat::A })
        );
        // Past the opening (no Mulligan offered), the verb resolves to nothing — a hint,
        // never a fabricated command.
        assert!(move_for_line("mull", &[Command::EndTurn], 5).is_err());
    }

    /// Character parity: the local CLI's bot opponent fields a NAMED character with a valid
    /// faction-pure deck. We assert it for BOTH factions across
    /// seeds: the picked character is a real roster member, and its deck passes `validate_deck_for`
    /// for that faction (legal size + singleton + faction purity). The bot now opens with a name and
    /// a real deck, exactly like the online seats.
    #[test]
    fn local_bot_fields_a_named_character_with_a_legal_deck() {
        use recollect_core::cards::validate_deck_for;
        let cat = canon_catalog();
        for seed in 0..16u64 {
            for faction in [Faction::Solace, Faction::Lorekeeper] {
                let opp = pick_bot_character(faction, seed, &cat);
                assert_eq!(opp.faction, faction);
                let roster: Vec<&str> = match faction {
                    Faction::Solace => SOLACE_CHARACTERS.iter().map(|c| c.name).collect(),
                    Faction::Lorekeeper => LOREKEEPER_CHARACTERS.iter().map(|c| c.name).collect(),
                };
                assert!(
                    roster.contains(&opp.name),
                    "{faction:?} seed {seed}: '{}' is not a roster character",
                    opp.name
                );
                validate_deck_for(&opp.deck, &cat, faction)
                    .unwrap_or_else(|e| panic!("{} ({faction:?}) seed {seed}: {e:?}", opp.name));
            }
        }
    }

    /// Determinism (the covenant): the same seed always faces the same character with the byte-for-byte
    /// same deck, so a `--seed` replays the exact match. The pick must also MATCH the server's
    /// matchmaking formula (`seed % roster.len()` indexing the roster, deck salted by `seed + 1`), so
    /// local and online field the same opponent for a given seed — true character parity, not a
    /// look-alike.
    #[test]
    fn local_bot_character_pick_is_deterministic_and_matches_matchmaking() {
        let cat = canon_catalog();
        for seed in [1u64, 7, 42, 1000, u64::MAX] {
            for faction in [Faction::Solace, Faction::Lorekeeper] {
                let a = pick_bot_character(faction, seed, &cat);
                let b = pick_bot_character(faction, seed, &cat);
                assert_eq!(
                    a.name, b.name,
                    "{faction:?} seed {seed}: pick not deterministic"
                );
                assert_eq!(
                    a.deck, b.deck,
                    "{faction:?} seed {seed}: deck not deterministic"
                );
                // Parity with the server: the same index formula + deck salt.
                match faction {
                    Faction::Solace => {
                        let i = (seed % SOLACE_CHARACTERS.len() as u64) as usize;
                        assert_eq!(a.name, SOLACE_CHARACTERS[i].name);
                        assert_eq!(a.deck, solace_character_deck(i, seed.wrapping_add(1), &cat));
                    }
                    Faction::Lorekeeper => {
                        let i = (seed % LOREKEEPER_CHARACTERS.len() as u64) as usize;
                        assert_eq!(a.name, LOREKEEPER_CHARACTERS[i].name);
                        assert_eq!(
                            a.deck,
                            lorekeeper_character_deck(i, seed.wrapping_add(1), &cat)
                        );
                    }
                }
            }
        }
    }

    /// The who-you-face announce names the opponent and reads in the right register per faction.
    #[test]
    fn opponent_intro_names_the_character_and_faction() {
        let solace = opponent_intro("Mara Quint", Faction::Solace);
        assert!(
            solace.contains("Mara Quint"),
            "names the character: {solace}"
        );
        assert!(solace.contains("Solace"), "Solace register: {solace}");
        let lk = opponent_intro("Archivist Pell", Faction::Lorekeeper);
        assert!(lk.contains("Archivist Pell"), "names the character: {lk}");
        assert!(lk.contains("Lorekeeper"), "Lorekeeper register: {lk}");
    }
}
