//! Reach geometry: orientation, reach tiles, placement projection, eff_defense.
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

pub(crate) fn oriented(reach: Reach, tile: u8, seat: Seat) -> Vec<u8> {
    oriented_w(reach, tile, seat, BOARD_W)
}

/// The tiles a spirit with `reach` at `tile` (facing for `seat`) can engage,
/// on a `w`-wide board. Public so UIs can preview a card's reach (highlight the
/// tiles it threatens). Pure geometry — no board state, no legality.
pub fn reach_tiles(reach: Reach, tile: u8, seat: Seat, width: u8) -> Vec<u8> {
    oriented_w(reach, tile, seat, width as i8)
}

pub(crate) fn oriented_w(reach: Reach, tile: u8, seat: Seat, w: i8) -> Vec<u8> {
    let (x, y) = tile_xy_w(tile, w);
    let f = seat.forward();
    reach
        .offsets()
        .iter()
        .filter_map(|(dx, dy)| xy_tile_w(x + dx, y + dy * f, w))
        .collect()
}

/// The aura-granted Reach extension reaching the spirit at `tile`: the sum of
/// `forward` steps and whether any aura grants "all directions", from the
/// reach-aura sources — a Landmark it stands on (OccupantHere), a bonded pair
/// (BondedPair, while adjacent), or an adjacent ally (AdjacentAlliesAll). All
/// canon reach auras are TARGETING-ONLY, so this is
/// consulted only by `targeting_reach`, never by projection/interception.
pub(crate) fn reach_aura_delta(sim: &GameState, catalog: &[CardDef], tile: u8) -> (i8, bool) {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let Some(me) = sim.spirit_at(tile) else {
        return (0, false);
    };
    let mut forward = 0i8;
    let mut all_dirs = false;
    let mut scan = |card_id: crate::types::CardId, want: S| {
        let Some(specs) = crate::effects::specs_for(catalog, card_id) else {
            return;
        };
        for spec in specs {
            if spec.trigger != Trigger::Static {
                continue;
            }
            for cl in &spec.clauses {
                if cl.selector == want
                    && let E::ReachDelta {
                        forward: f,
                        all_directions,
                        ..
                    } = &cl.effect
                {
                    forward += *f;
                    all_dirs |= *all_directions;
                }
            }
        }
    };
    // Landmark the spirit stands on.
    if let Some(terr) = &sim.board[tile as usize].terrain
        && !terr.face_down
    {
        scan(terr.card, S::OccupantHere);
    }
    // A bonded pair the spirit belongs to (while present & adjacent).
    for b in &sim.bonds {
        if (b.tile_a == tile || b.tile_b == tile)
            && manhattan(b.tile_a, b.tile_b) == 1
            && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
            && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
        {
            scan(b.card, S::BondedPair);
        }
    }
    // Adjacent allies whose standing aura grants reach (Pathfinder Ibex).
    for (i, t) in sim.board.iter().enumerate() {
        if let Some(sp) = &t.spirit
            && !sp.fading
            && sp.owner == me.owner
            && i as u8 != tile
            && manhattan(i as u8, tile) == 1
        {
            scan(sp.card, S::AdjacentAlliesAll);
        }
    }
    // Roc Paramount: a standing aura grants reach to ALL allies, board-wide (including
    // itself) — scan every allied spirit for an AlliesAll reach grant.
    for t in sim.board.iter() {
        if let Some(sp) = &t.spirit
            && !sp.fading
            && sp.owner == me.owner
        {
            scan(sp.card, S::AlliesAll);
        }
    }
    // This-round reach buffs covering this spirit (seat-wide `tile: None`, or
    // per-spirit `tile: Some`). ALL buffs widen TARGETING (both targeting-only and
    // full); projection/interception consult only the full ones (`full_reach_delta`).
    for tr in &sim.temp_reach {
        if tr.seat == me.owner
            && tr.until_round >= sim.round
            && tr.tile.map(|t| t == tile).unwrap_or(true)
        {
            forward += tr.forward;
            all_dirs |= tr.all_directions;
        }
    }
    (forward, all_dirs)
}

/// The FULL (non-targeting-only) reach buffs covering the spirit at `tile` — the
/// part of `temp_reach` that also widens projection and interception (Tailwind,
/// Open Sky). Seat-wide (`tile: None`) or per-spirit (`tile: Some`).
pub(crate) fn full_reach_delta(sim: &GameState, tile: u8, seat: Seat) -> (i8, bool) {
    let mut forward = 0i8;
    let mut all_dirs = false;
    for tr in &sim.temp_reach {
        if !tr.targeting_only
            && tr.seat == seat
            && tr.until_round >= sim.round
            && tr.tile.map(|t| t == tile).unwrap_or(true)
        {
            forward += tr.forward;
            all_dirs |= tr.all_directions;
        }
    }
    (forward, all_dirs)
}

/// Widen a base oriented reach set by `(forward, all_dirs)` — the one-step frontier
/// extension shared by `targeting_reach` and the projection/interception sites.
pub(crate) fn widen_oriented(
    mut tiles: Vec<u8>,
    forward: i8,
    all_dirs: bool,
    seat: Seat,
    w: i8,
) -> Vec<u8> {
    if forward == 0 && !all_dirs {
        return tiles;
    }
    let f = seat.forward();
    // Dedup via a bitset over the ≤36 board tiles (every tile id fits a u64 bit), so the
    // frontier extension is allocation-free and avoids the prior O(n²) linear `contains`.
    // The base set never repeats a tile, so seeding the mask from it is exact.
    let mut seen: u64 = 0;
    for &t in &tiles {
        seen |= 1 << t;
    }
    // Snapshot the base length: only the ORIGINAL tiles spawn neighbours (the new tiles we
    // append must not themselves be widened — that would over-extend the frontier).
    let base_len = tiles.len();
    for idx in 0..base_len {
        let (bx, by) = tile_xy_w(tiles[idx], w);
        for step in 1..=forward {
            if let Some(nt) = xy_tile_w(bx, by + step * f, w)
                && seen & (1 << nt) == 0
            {
                seen |= 1 << nt;
                tiles.push(nt);
            }
        }
        if all_dirs {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if let Some(nt) = xy_tile_w(bx + dx, by + dy, w)
                    && seen & (1 << nt) == 0
                {
                    seen |= 1 << nt;
                    tiles.push(nt);
                }
            }
        }
    }
    tiles
}

/// Whether a this-round, seat-wide restriction `r` is in force for `owner`
/// (Stand Ground). Pure read of `temp_restrict`, guarded by the current round.
pub(crate) fn restricted(sim: &GameState, owner: Seat, r: crate::effects::Restriction) -> bool {
    sim.temp_restrict
        .iter()
        .any(|t| t.seat == owner && t.restriction == r && t.until_round >= sim.round)
}

/// Reach for ARRIVAL TARGETING (engage/strike): the base oriented reach, widened
/// by any reach aura at `tile`. Returns the base set unchanged when no aura is
/// present (so a call site is behaviour-identical absent a reach aura). Geometry
/// of the widening: each base tile gains its forward neighbour (`forward` N) and,
/// for "all directions", its four orthogonal neighbours — a one-step frontier
/// extension, off-board and self tiles dropped.
pub(crate) fn targeting_reach(
    sim: &GameState,
    catalog: &[CardDef],
    base: Reach,
    tile: u8,
    seat: Seat,
    w: i8,
) -> Vec<u8> {
    // The Almost-Said: a spirit that copied an engager's Reach uses that Reach, not its own.
    let base = sim
        .spirit_at(tile)
        .and_then(|s| s.copied_reach)
        .unwrap_or(base);
    let tiles = oriented_w(base, tile, seat, w);
    let (forward, all_dirs) = reach_aura_delta(sim, catalog, tile);
    let mut tiles = widen_oriented(tiles, forward, all_dirs, seat, w);
    tiles.retain(|&nt| nt != tile);
    tiles
}

/// Rooted Telling: reach of your spirits ∪ adjacency of your impressions ∪ your
/// home rows (while they exist). Landmarks join in M-next.
/// Team-shared by construction: `seat` is the TEAM, so both teammates'
/// spirits (owner == seat) feed one projection. Width-aware for 2v2.
/// The projection a single SLOT may write from. In 1v1 this equals
/// the team projection. In 2v2 it counts only the spirits THIS slot placed
/// (placed_by == slot) plus the shared impressions, terrain, and home rows — a
/// partner's reach does not authorize your Overwrite. Used by Overwrite only;
/// PLACEMENT still uses the team projection (teammates build a shared front).
pub fn projection_slot(
    st: &GameState,
    slot: crate::types::SeatSlot,
    catalog: &[CardDef],
) -> Vec<bool> {
    if !st.is_2v2() {
        return projection(st, slot.team(), catalog);
    }
    let w = st.board_w;
    let seat = slot.team();
    let mut p = vec![false; st.board.len()];
    for (i, t) in st.board.iter().enumerate() {
        if let Some(sp) = &t.spirit {
            // Only YOUR OWN spirits project for your Overwrite right.
            if sp.placed_by == Some(slot) && !sp.fading && !sp.face_down {
                let reach = card(catalog, sp.card).reach;
                let (fwd, ad) = full_reach_delta(st, i as u8, seat);
                for r in widen_oriented(oriented_w(reach, i as u8, seat, w), fwd, ad, seat, w) {
                    p[r as usize] = true;
                }
            }
        }
        // Impressions and terrain are shared team assets — they still project.
        if t.impressions.contains(&seat) {
            for a in adjacent4_w(i as u8, w) {
                p[a as usize] = true;
            }
        }
        if let Some(terr) = &t.terrain
            && terr.owner == seat
        {
            for a in adjacent4_w(i as u8, w) {
                p[a as usize] = true;
            }
        }
    }
    if !st.contracted {
        for row in seat.home_rows_w(w) {
            for x in 0..w {
                p[xy_tile_w(x, row, w).unwrap() as usize] = true;
            }
        }
    }
    p
}

/// The tiles `seat` (a TEAM) may write to: every standing own spirit's reach,
/// plus tiles adjacent to the team's impressions and owned terrain, plus the team's
/// home rows (until contraction). Used for placement. Overwrite uses the
/// stricter per-slot [`projection_slot`] in 2v2.
pub fn projection(st: &GameState, seat: Seat, catalog: &[CardDef]) -> Vec<bool> {
    let w = st.board_w;
    let mut p = vec![false; st.board.len()];
    for (i, t) in st.board.iter().enumerate() {
        if let Some(sp) = &t.spirit
            && sp.owner == seat
            && !sp.fading
            && !sp.face_down
        {
            let reach = card(catalog, sp.card).reach;
            // A FULL reach buff (Tailwind, Open Sky) widens placement too.
            let (fwd, ad) = full_reach_delta(st, i as u8, seat);
            for r in widen_oriented(oriented_w(reach, i as u8, seat, w), fwd, ad, seat, w) {
                p[r as usize] = true;
            }
        }
        if t.impressions.contains(&seat) {
            for a in adjacent4_w(i as u8, w) {
                p[a as usize] = true;
            }
        }
        if let Some(terr) = &t.terrain
            && terr.owner == seat
        {
            for a in adjacent4_w(i as u8, w) {
                p[a as usize] = true;
            }
        }
    }
    if !st.contracted {
        for row in seat.home_rows_w(w) {
            for x in 0..w {
                p[xy_tile_w(x, row, w).unwrap() as usize] = true;
            }
        }
    }
    p
}

pub(crate) fn any_projected_placement(st: &GameState, seat: Seat, catalog: &[CardDef]) -> bool {
    let p = projection(st, seat, catalog);
    // A legal placement needs a projected tile that is unfaded, spirit-free AND
    // terrain-free (a spirit may not land on a Landmark / Fabrication). This agrees with
    // `decide_play_spirit`'s reject, so the Margin Rule isn't wrongly suppressed by a
    // tile that only holds terrain.
    st.board
        .iter()
        .enumerate()
        .any(|(i, t)| p[i] && !t.faded && t.spirit.is_none() && t.terrain.is_none())
}

/// Effective defense in an exchange: Arcane attackers ignore defense; Warded
/// defenders keep theirs against non-Arcane. The single chokepoint for the
/// Arcane/Warded interaction so combat and previews agree.
pub(crate) fn eff_defense(def_defense: i16, attacker_arcane: bool, defender_warded: bool) -> i16 {
    if attacker_arcane && !defender_warded {
        (def_defense - ARCANE_PIERCE).max(0)
    } else {
        def_defense
    }
}

#[cfg(test)]
mod math_props {
    use super::*;
    // Bounded-exhaustive properties of the pure defense math, swept over the whole
    // realistic stat domain. This is the formal-math safety net: under
    // `cargo kani` these become unbounded proofs; here the sweep is exact for i16
    // defenses in range. See docs/testing.md "Invariants registry".
    #[test]
    fn eff_defense_properties_hold_across_the_stat_domain() {
        for def in 0i16..=300 {
            // Non-arcane attacker: full defense applies, warded or not.
            assert_eq!(eff_defense(def, false, false), def);
            assert_eq!(eff_defense(def, false, true), def);
            // Warded defender negates the arcane pierce entirely.
            assert_eq!(
                eff_defense(def, true, true),
                def,
                "Warded must stop arcane pierce at def={def}"
            );
            // Arcane + unwarded pierces exactly ARCANE_PIERCE, clamped at 0, never raising.
            let pierced = eff_defense(def, true, false);
            assert_eq!(pierced, (def - ARCANE_PIERCE).max(0));
            assert!(
                (0..=def).contains(&pierced),
                "pierce out of range at def={def}"
            );
        }
        // Monotonic: a higher base defense never yields a lower effective defense.
        for def in 1i16..=300 {
            for &(a, w) in &[(false, false), (true, false), (true, true)] {
                assert!(eff_defense(def, a, w) >= eff_defense(def - 1, a, w));
            }
        }
    }

    /// The pre-perf `widen_oriented` body, verbatim: an `extra` Vec collecting every
    /// frontier neighbour (duplicates allowed), then a dedup-while-appending pass via
    /// `Vec::contains` against the GROWING `tiles`. The current implementation replaced
    /// this with a `u64` bitset dedup + immediate push (commit 921aab7). This twin lets the
    /// property test below assert the two produce **byte-identical** output (same tiles,
    /// SAME ORDER) — the off-by-one / ordering guard the perf commit needs, since a
    /// reordered frontier would silently change legal-move enumeration.
    fn widen_oriented_legacy(
        mut tiles: Vec<u8>,
        forward: i8,
        all_dirs: bool,
        seat: Seat,
        w: i8,
    ) -> Vec<u8> {
        if forward == 0 && !all_dirs {
            return tiles;
        }
        let f = seat.forward();
        let mut extra: Vec<u8> = Vec::new();
        for &t in &tiles {
            let (bx, by) = tile_xy_w(t, w);
            for step in 1..=forward {
                if let Some(nt) = xy_tile_w(bx, by + step * f, w) {
                    extra.push(nt);
                }
            }
            if all_dirs {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if let Some(nt) = xy_tile_w(bx + dx, by + dy, w) {
                        extra.push(nt);
                    }
                }
            }
        }
        for nt in extra {
            if !tiles.contains(&nt) {
                tiles.push(nt);
            }
        }
        tiles
    }

    /// The bitset-dedup `widen_oriented` must equal the legacy `Vec::contains` twin for
    /// EVERY (base set, forward, all_dirs, seat, width) over both board widths — same
    /// tiles in the same order. Swept exhaustively over single-tile and contiguous
    /// multi-tile base sets, every `forward` a reach can carry (0..=2), both `all_dirs`,
    /// both seats, both widths. A divergence here would mean the perf swap reshaped a
    /// reach frontier (an extra/missing/reordered legal tile).
    #[test]
    fn widen_oriented_matches_the_legacy_contains_dedup_byte_for_byte() {
        for &w in &[crate::types::BOARD_W, crate::types::BOARD_W_2V2] {
            let ntiles = (w as u16 * w as u16) as u8;
            for &seat in &[Seat::A, Seat::B] {
                for forward in 0i8..=2 {
                    for &all_dirs in &[false, true] {
                        // (a) every single-tile base.
                        for t0 in 0..ntiles {
                            let base = vec![t0];
                            let got = widen_oriented(base.clone(), forward, all_dirs, seat, w);
                            let want =
                                widen_oriented_legacy(base.clone(), forward, all_dirs, seat, w);
                            assert_eq!(
                                got, want,
                                "single-tile base {t0}, fwd={forward}, all_dirs={all_dirs}, \
                                 seat={seat:?}, w={w}"
                            );
                        }
                        // (b) contiguous pairs and triples (overlapping frontiers stress the
                        // cross-tile dedup — the case where order actually matters).
                        for t0 in 0..ntiles {
                            for len in 2..=3u8 {
                                let base: Vec<u8> = (t0..(t0 + len).min(ntiles)).collect();
                                if base.len() < 2 {
                                    continue;
                                }
                                let got = widen_oriented(base.clone(), forward, all_dirs, seat, w);
                                let want =
                                    widen_oriented_legacy(base.clone(), forward, all_dirs, seat, w);
                                assert_eq!(
                                    got, want,
                                    "base {base:?}, fwd={forward}, all_dirs={all_dirs}, \
                                     seat={seat:?}, w={w}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
