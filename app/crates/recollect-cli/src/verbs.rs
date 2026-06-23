//! The shared verb grammar — the keyboard shortcut language both human CLI modes
//! speak. A move is *always* available by **number** from the legal-move menu;
//! the verbs are an optional faster path for players who know what they want.
//!
//! ONE parser, used by both transports: the networked TUI ([`crate::online`])
//! and local play ([`crate::local`]) share this exactly — no duplication, no drift.
//! It mirrors the web's input vocabulary: pick a spirit / card, name a
//! destination, optionally engage; Glimpse, Release, Evolve, Reclaim, End turn.
//!
//! ## Tiles, two ways
//! A tile token is accepted **either** as a raw board **index** (`12`) **or** as a
//! grid **coordinate** (`c3` — column `c`, row `3`). The networked menu speaks
//! indices and the local board prints coordinates, so each mode's players type
//! what they already see; the parser takes both regardless of mode.
//!
//! ## The grammar
//! | Verb | Action |
//! |---|---|
//! | `p <hand#> <tile> [e <tile>]` | **Play** a spirit (optionally engaging on arrival) |
//! | `o <hand#> <tile>` | **Overwrite** a tile with a hand card |
//! | `m <from> <to> [e <tile>]` | **Move** a spirit (optionally engaging) |
//! | `v <tile>` / `evolve <tile>` | **Evolve** the base on `<tile>` (by playing a held form card onto it) |
//! | `dv <tile>` / `devolve <tile>` | **Devolve** the standing-Faded form on `<tile>` (the rescue — Lorekeeper *reverts*, Solace *recedes*) |
//! | `g` / `glimpse` | **Glimpse** |
//! | `r <hand#>` | **Release** a hand card (when the hand is full at Flow) |
//! | `rc <tile>` / `reclaim <tile>` | **Reclaim** a standing spirit for Anima |
//! | `end` | **End turn** |
//!
//! `Evolve` is intentionally underspecified by the verb (`form_hand`/`fuel` are
//! left to the caller to resolve against the legal set) — the verb only *names the
//! base tile*; the caller disambiguates to a concrete legal `Evolve` (the held form
//! card + any donor). This keeps the parser a pure, engine-free string→intent map.
use recollect_core::Command;

/// A parsed verb intent. Most map straight to a [`Command`]; `Evolve`/`Reclaim`
/// name only the tile and are resolved against the legal set by the caller (a
/// base may have several legal Evolves — one per held form card — so the menu still
/// disambiguates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// A fully-specified command (Play / Overwrite / Move / Glimpse / Release / End).
    Cmd(Command),
    /// Evolve the base on this tile — resolved to a concrete legal `Evolve` by the
    /// caller (a held form card played onto its base).
    Evolve { tile: u8 },
    /// Devolve (§5) the standing-Faded form on this tile — the rescue. Names only the
    /// tile; resolved to the legal `Devolve` (the base card in hand) by the caller (a
    /// form may have several legal Devolves — one per held base copy — so the numbered
    /// menu still disambiguates). The Lorekeeper *reverts*, the Solace *recedes*.
    Devolve { tile: u8 },
    /// Reclaim the standing spirit on this tile — resolved to the legal `Reclaim`.
    Reclaim { tile: u8 },
    /// Mulligan the opening hand (§5) — names no seat; resolved to the legal
    /// `Mulligan { seat }` for the active seat (offered only in the opening window).
    Mulligan,
}

/// Parse a tile token: a raw index (`12`) or a grid coordinate (`c3`). `w` is the
/// board width (5 for 1v1, 6 for 2v2). Returns `None` for anything off-board.
pub fn parse_tile(s: &str, w: u8) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<u8>() {
        return ((n as usize) < (w as usize * w as usize)).then_some(n);
    }
    let b = s.as_bytes();
    if b.len() == 2 {
        let col = b[0].to_ascii_lowercase().wrapping_sub(b'a');
        let row = b[1].wrapping_sub(b'1');
        if (col as usize) < w as usize && (row as usize) < w as usize {
            return Some(row * w + col);
        }
    }
    None
}

/// Parse one line of the verb grammar against a board of width `w`. Returns the
/// [`Intent`], or `None` if the line is not a (well-formed) verb — the caller then
/// reports the usage hint. Tiles accept both index and `c3` grid forms.
pub fn parse(line: &str, w: u8) -> Option<Intent> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let t = |s: &str| parse_tile(s, w);
    let n = |s: &str| s.parse::<u8>().ok();
    // An optional trailing `e <tile>` engage clause starting at word `at`.
    let engage = |words: &[&str], at: usize| -> Option<u8> {
        if words.len() > at + 1 && words[at] == "e" {
            t(words[at + 1])
        } else {
            None
        }
    };
    match words.as_slice() {
        ["p", h, tile, ..] => Some(Intent::Cmd(Command::PlaySpirit {
            hand_index: n(h)?,
            tile: t(tile)?,
            engage: engage(&words, 3),
            chain_prefs: Vec::new(),
        })),
        ["o", h, tile] => Some(Intent::Cmd(Command::Overwrite {
            hand_index: n(h)?,
            tile: t(tile)?,
        })),
        ["m", f, to, ..] => Some(Intent::Cmd(Command::MoveSpirit {
            from: t(f)?,
            to: t(to)?,
            engage: engage(&words, 3),
        })),
        ["v", tile] | ["evolve", tile] => Some(Intent::Evolve { tile: t(tile)? }),
        ["dv", tile] | ["devolve", tile] => Some(Intent::Devolve { tile: t(tile)? }),
        ["rc", tile] | ["reclaim", tile] => Some(Intent::Reclaim { tile: t(tile)? }),
        ["g"] | ["glimpse"] => Some(Intent::Cmd(Command::Glimpse)),
        ["r", h] => Some(Intent::Cmd(Command::Release { hand_index: n(h)? })),
        ["end"] => Some(Intent::Cmd(Command::EndTurn)),
        ["mull"] | ["mulligan"] => Some(Intent::Mulligan),
        _ => None,
    }
}

/// The one-line usage hint, shown when a typed line isn't a valid verb.
pub const USAGE: &str = "verbs: p <hand#> <tile> [e <tile>] · o <hand#> <tile> · m <from> <to> [e <tile>] · v <tile> (Evolve) · dv <tile> (Devolve — revert/recede a faded form) · rc <tile> (Reclaim) · g (Glimpse) · r <hand#> · mull (Mulligan, opening only) · end · q";

/// Resolve a verb [`Intent`] to a concrete [`Command`] from the `legal` set. A
/// direct `Intent::Cmd` is returned as-is (the engine validates it anyway);
/// `Evolve`/`Reclaim` name only a tile, so they're matched to the legal command on
/// that tile (the first legal form/fuel — the numbered menu remains the way to pick
/// among several forms). Returns `None` if nothing legal matches the named tile.
pub fn resolve(intent: Intent, legal: &[Command]) -> Option<Command> {
    match intent {
        Intent::Cmd(c) => Some(c),
        Intent::Evolve { tile } => legal
            .iter()
            .find(|c| matches!(c, Command::Evolve { tile: et, .. } if *et == tile))
            .cloned(),
        Intent::Devolve { tile } => legal
            .iter()
            .find(|c| matches!(c, Command::Devolve { tile: dt, .. } if *dt == tile))
            .cloned(),
        Intent::Reclaim { tile } => legal
            .iter()
            .find(|c| matches!(c, Command::Reclaim { tile: rt } if *rt == tile))
            .cloned(),
        Intent::Mulligan => legal
            .iter()
            .find(|c| matches!(c, Command::Mulligan { .. }))
            .cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_move_overwrite_parse_with_index_tiles() {
        assert_eq!(
            parse("p 0 12 e 13", 5),
            Some(Intent::Cmd(Command::PlaySpirit {
                hand_index: 0,
                tile: 12,
                engage: Some(13),
                chain_prefs: Vec::new(),
            }))
        );
        assert_eq!(
            parse("o 1 7", 5),
            Some(Intent::Cmd(Command::Overwrite {
                hand_index: 1,
                tile: 7,
            }))
        );
        assert_eq!(
            parse("m 6 7", 5),
            Some(Intent::Cmd(Command::MoveSpirit {
                from: 6,
                to: 7,
                engage: None,
            }))
        );
        assert_eq!(parse("end", 5), Some(Intent::Cmd(Command::EndTurn)));
        assert_eq!(parse("g", 5), Some(Intent::Cmd(Command::Glimpse)));
        assert_eq!(parse("glimpse", 5), Some(Intent::Cmd(Command::Glimpse)));
        assert_eq!(parse("dance", 5), None);
    }

    #[test]
    fn tiles_accept_both_grid_coords_and_indices() {
        // c3 == column c (x=2), row 3 (y=2) == 2*5+2 == 12.
        assert_eq!(
            parse("p 0 c3", 5),
            Some(Intent::Cmd(Command::PlaySpirit {
                hand_index: 0,
                tile: 12,
                engage: None,
                chain_prefs: Vec::new(),
            }))
        );
        // Grid coords in a Move, including the engage clause.
        assert_eq!(
            parse("m a1 b1 e c3", 5),
            Some(Intent::Cmd(Command::MoveSpirit {
                from: 0,
                to: 1,
                engage: Some(12),
            }))
        );
        assert_eq!(parse_tile("a1", 5), Some(0));
        assert_eq!(parse_tile("12", 5), Some(12));
        assert_eq!(parse_tile("z9", 5), None);
        assert_eq!(parse_tile("99", 5), None);
    }

    #[test]
    fn evolve_and_reclaim_name_a_tile() {
        assert_eq!(parse("v c3", 5), Some(Intent::Evolve { tile: 12 }));
        assert_eq!(parse("evolve 7", 5), Some(Intent::Evolve { tile: 7 }));
        assert_eq!(parse("rc c3", 5), Some(Intent::Reclaim { tile: 12 }));
        assert_eq!(parse("reclaim 7", 5), Some(Intent::Reclaim { tile: 7 }));
    }

    #[test]
    fn devolve_verb_parses_and_resolves_to_the_legal_command_on_the_tile() {
        // `dv`/`devolve` name only the faded-form tile; both forms parse.
        assert_eq!(parse("dv c3", 5), Some(Intent::Devolve { tile: 12 }));
        assert_eq!(parse("devolve 7", 5), Some(Intent::Devolve { tile: 7 }));
        // It resolves to the legal `Devolve` on that tile (the base card from the menu).
        let legal = vec![
            Command::Devolve {
                tile: 12,
                base_hand: 1,
            },
            Command::EndTurn,
        ];
        assert_eq!(
            resolve(Intent::Devolve { tile: 12 }, &legal),
            Some(Command::Devolve {
                tile: 12,
                base_hand: 1,
            })
        );
        // No legal Devolve on tile 3 ⇒ nothing resolves (caller reports it).
        assert_eq!(resolve(Intent::Devolve { tile: 3 }, &legal), None);
    }

    #[test]
    fn resolve_matches_evolve_and_reclaim_against_the_legal_set() {
        let legal = vec![
            Command::Evolve {
                tile: 12,
                form_hand: 0,
                fuel: None,
                engage: None,
            },
            Command::Reclaim { tile: 7 },
            Command::EndTurn,
        ];
        assert_eq!(
            resolve(Intent::Evolve { tile: 12 }, &legal),
            Some(legal[0].clone())
        );
        assert_eq!(
            resolve(Intent::Reclaim { tile: 7 }, &legal),
            Some(Command::Reclaim { tile: 7 })
        );
        // No legal Evolve on tile 3 ⇒ nothing resolves (caller reports it).
        assert_eq!(resolve(Intent::Evolve { tile: 3 }, &legal), None);
        // A direct command passes through untouched.
        assert_eq!(
            resolve(Intent::Cmd(Command::Glimpse), &legal),
            Some(Command::Glimpse)
        );
    }

    #[test]
    fn mulligan_verb_parses_and_resolves_to_the_legal_seat_command() {
        use recollect_core::Seat;
        // `mull`/`mulligan` both parse to the seat-less intent.
        assert_eq!(parse("mull", 5), Some(Intent::Mulligan));
        assert_eq!(parse("mulligan", 5), Some(Intent::Mulligan));
        // It resolves to the legal `Mulligan { seat }` (the seat comes from the menu).
        let legal = vec![Command::Mulligan { seat: Seat::A }, Command::EndTurn];
        assert_eq!(
            resolve(Intent::Mulligan, &legal),
            Some(Command::Mulligan { seat: Seat::A })
        );
        // With no legal mulligan (past the opening), nothing resolves.
        assert_eq!(resolve(Intent::Mulligan, &[Command::EndTurn]), None);
    }
}
