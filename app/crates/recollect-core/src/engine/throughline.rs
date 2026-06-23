//! Throughline: the line-of-loved-spirits chain — imprint links, the run/grant,
//! and the +10/+10 throughline reward riders. A sibling of `clause.rs`.
use super::*;

/// Imprints the spirit at `t` carries for Throughline matching: its own (unless
/// traits-stripped), plus a Twin-Telling-bonded partner's (PairSharesImprints).
pub(crate) fn throughline_imprints(sim: &GameState, catalog: &[CardDef], t: u8) -> Vec<String> {
    use crate::effects::RuleException::PairSharesImprints;
    let Some(sp) = sim.spirit_at(t) else {
        return Vec::new();
    };
    let mut ims = if sp.traits_blanked(sim.round) {
        Vec::new()
    } else {
        card(catalog, sp.card).imprints.clone()
    };
    for b in &sim.bonds {
        let other = if b.tile_a == t {
            Some(b.tile_b)
        } else if b.tile_b == t {
            Some(b.tile_a)
        } else {
            None
        };
        if let Some(o) = other
            && card_carries_static_exception(catalog, b.card, PairSharesImprints)
            && let Some(osp) = sim.spirit_at(o).filter(|s| !s.traits_blanked(sim.round))
        {
            ims.extend(card(catalog, osp.card).imprints.iter().cloned());
        }
    }
    ims
}

/// The spirit at `t` is an allied, non-fading Throughline link carrying `im` — or a
/// wildcard (Errata: AllImprintsShared "counts as every Imprint").
pub(crate) fn throughline_link(
    sim: &GameState,
    catalog: &[CardDef],
    t: u8,
    owner: Seat,
    im: &str,
) -> bool {
    use crate::effects::RuleException::AllImprintsShared;
    let Some(sp) = sim.spirit_at(t) else {
        return false;
    };
    if sp.fading || sp.owner != owner {
        return false;
    }
    card_carries_static_exception(catalog, sp.card, AllImprintsShared)
        || throughline_imprints(sim, catalog, t)
            .iter()
            .any(|i| i == im)
}

/// Two spirits 2 tiles apart are bridged for a Throughline by an Unbreakable bond
/// (BondHoldsUnlessSeparated) joining them.
pub(crate) fn unbreakable_bridges(sim: &GameState, catalog: &[CardDef], a: u8, b: u8) -> bool {
    use crate::effects::RuleException::BondHoldsUnlessSeparated;
    sim.bonds.iter().any(|bd| {
        ((bd.tile_a == a && bd.tile_b == b) || (bd.tile_a == b && bd.tile_b == a))
            && card_carries_static_exception(catalog, bd.card, BondHoldsUnlessSeparated)
    })
}

/// Length of the straight Throughline run through `tile` along axis (dx,dy) for imprint
/// `im`, allowing one Unbreakable-bridged 1-tile gap per direction.
pub(crate) fn throughline_run(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    owner: Seat,
    im: &str,
    dx: i8,
    dy: i8,
) -> usize {
    let w = sim.board_w;
    let mut count = 1usize;
    for dir in [1i8, -1i8] {
        let mut gap_used = false;
        let mut prev = tile;
        loop {
            let (px, py) = crate::types::tile_xy_w(prev, w);
            let (nx, ny) = (px + dx * dir, py + dy * dir);
            let Some(next) = crate::types::xy_tile_w(nx, ny, w) else {
                break;
            };
            if throughline_link(sim, catalog, next, owner, im) {
                count += 1;
                prev = next;
                continue;
            }
            if !gap_used
                && sim.spirit_at(next).is_none()
                && let Some(beyond) = crate::types::xy_tile_w(nx + dx * dir, ny + dy * dir, w)
                && throughline_link(sim, catalog, beyond, owner, im)
                && unbreakable_bridges(sim, catalog, prev, beyond)
            {
                gap_used = true;
                count += 1;
                prev = beyond;
                continue;
            }
            break;
        }
    }
    count
}

/// after an arrival/move, complete a Throughline if `tile` now forms a straight line
/// of 3+ allied spirits sharing one Imprint (Errata wildcard; Twin Telling pools; Unbreakable
/// bridges one gap). The completing spirit gains +10/+10 and a full restore, once.
pub(crate) fn check_throughline(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    tile: u8,
    owner: Seat,
) {
    let Some(sp) = sim.spirit_at(tile) else {
        return;
    };
    if sp.fading || sp.owner != owner || sp.throughline_done {
        return;
    }
    // Candidate imprints: tile's own, plus its orthogonal allied neighbours' (so an Errata
    // wildcard can complete on whatever Imprint its line uses).
    let mut cands = throughline_imprints(sim, catalog, tile);
    let (cx, cy) = crate::types::tile_xy_w(tile, sim.board_w);
    for (dx, dy) in [(1i8, 0i8), (-1, 0), (0, 1), (0, -1)] {
        if let Some(n) = crate::types::xy_tile_w(cx + dx, cy + dy, sim.board_w)
            && sim
                .spirit_at(n)
                .map(|s| !s.fading && s.owner == owner)
                .unwrap_or(false)
        {
            cands.extend(throughline_imprints(sim, catalog, n));
        }
    }
    cands.sort();
    cands.dedup();
    for im in &cands {
        for (dx, dy) in [(1i8, 0i8), (0i8, 1i8)] {
            if throughline_run(sim, catalog, tile, owner, im, dx, dy) >= 3 {
                // Queen of the Quiet Garden amplifies the completion buff.
                let (qa, qd) = throughline_grant(sim, catalog, owner);
                push(
                    sim,
                    evs,
                    Event::ThroughlineCompleted {
                        tile,
                        attack: 10 + qa,
                        defense: 10 + qd,
                    },
                );
                // Vale Eternal: fire the owner's "on Throughline complete" riders.
                fire_throughline_riders(sim, evs, catalog, owner, tile);
                return;
            }
        }
    }
}

/// Queen of the Quiet Garden: the EXTRA Throughline buff `seat` grants (summed over its
/// standing Queens, via Static Owner/ThroughlineGrant).
pub(crate) fn throughline_grant(sim: &GameState, catalog: &[CardDef], seat: Seat) -> (i16, i16) {
    let mut a = 0;
    let mut d = 0;
    for t in &sim.board {
        let Some(sp) = &t.spirit else { continue };
        if sp.owner != seat || sp.fading || sp.face_down {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, sp.card) {
            for s in specs
                .iter()
                .filter(|s| s.trigger == crate::effects::Trigger::Static)
            {
                for cl in &s.clauses {
                    if let crate::effects::Effect::ThroughlineGrant { attack, defense } = cl.effect
                    {
                        a += attack;
                        d += defense;
                    }
                }
            }
        }
    }
    (a, d)
}

/// Vale Eternal: when one of `seat`'s Throughlines completes, fire each of the seat's
/// standing OnThroughlineComplete riders (draw / Anima).
pub(crate) fn fire_throughline_riders(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    seat: Seat,
    completed_tile: u8,
) {
    let riders: Vec<(u8, String)> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let sp = t.spirit.as_ref()?;
            if sp.owner != seat || sp.fading || sp.face_down {
                return None;
            }
            let name = card(catalog, sp.card).name.clone();
            crate::effects::specs_for(catalog, sp.card)?
                .iter()
                .any(|s| s.trigger == crate::effects::Trigger::OnThroughlineComplete)
                .then_some((i as u8, name))
        })
        .collect();
    for (rider_tile, name) in riders {
        let _ = completed_tile; // riders are Owner-scoped; the completer is just context
        fire_effects_noctx(
            sim,
            evs,
            catalog,
            &name,
            crate::effects::Trigger::OnThroughlineComplete,
            Some(rider_tile),
            seat,
        );
    }
}
