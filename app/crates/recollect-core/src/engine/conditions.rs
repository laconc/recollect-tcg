//! Geometry + condition predicates (manhattan, condition_holds, shared-resonance).
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

pub(crate) fn manhattan(a: u8, b: u8) -> i16 {
    let (ax, ay) = tile_xy(a);
    let (bx, by) = tile_xy(b);
    (ax as i16 - bx as i16).abs() + (ay as i16 - by as i16).abs()
}

/// Catalog-free Attune check used inside condition_holds (counts by card id
/// resonance via the canon catalog is done in combat_stats; here a
/// conservative structural check suffices because Attune specs pair the
/// condition with GrantSharedResonance which re-derives with the catalog).
pub(crate) fn shared_adjacent_resonance_cond(
    sim: &GameState,
    tile: u8,
    sp: &Spirit,
    _n: u8,
) -> bool {
    sim.board.iter().enumerate().any(|(i, t)| {
        i as u8 != tile
            && manhattan(tile, i as u8) == 1
            && t.spirit
                .as_ref()
                .map(|o| !o.fading && o.owner == sp.owner)
                .unwrap_or(false)
    })
}

/// Whether an effect clause's `Condition` is met (e.g. WhileDamaged, a round
/// gate). Static auras consult this each read; instants check it once at fire.
pub(crate) fn condition_holds(
    sim: &GameState,
    cond: &crate::effects::Condition,
    tile: u8,
    sp: &Spirit,
) -> bool {
    use crate::effects::Condition as C;
    match cond {
        C::Always => true,
        C::WhileDamaged => sp.hp < sp.hp_max,
        C::WhileUndamaged => sp.hp >= sp.hp_max,
        C::WhileAdjacentToAlly => sim.board.iter().enumerate().any(|(i, t)| {
            t.spirit
                .as_ref()
                .map(|o| {
                    !o.fading
                        && o.owner == sp.owner
                        && manhattan(tile, i as u8) == 1
                        && i as u8 != tile
                })
                .unwrap_or(false)
        }),
        C::AdjacentAlliesShareResonance { n } => shared_adjacent_resonance_cond(sim, tile, sp, *n),
        C::OncePerMatch => !sp.replacement_used,
        C::WhileFaceDown => sp.face_down,
        _ => false,
    }
}

pub(crate) fn shared_adjacent_resonance(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    sp: &Spirit,
    n: u8,
) -> Option<Resonance> {
    // Attune: adjacent to n+ allies sharing a Resonance.
    let mut counts: Vec<(Resonance, u8)> = Vec::new();
    for (i, t) in sim.board.iter().enumerate() {
        if i as u8 == tile || manhattan(tile, i as u8) != 1 {
            continue;
        }
        if let Some(o) = &t.spirit {
            if o.fading || o.owner != sp.owner {
                continue;
            }
            let r = card(catalog, o.card).resonance;
            match counts.iter_mut().find(|(cr, _)| *cr == r) {
                Some((_, c)) => *c += 1,
                None => counts.push((r, 1)),
            }
        }
    }
    counts.into_iter().find(|(_, c)| *c >= n).map(|(r, _)| r)
}

// Effective combat numbers for the spirit at `tile`, optionally in the
// context of a strike against `vs` (Marrow-class engage auras).

/// Test-visible view of a tile's derived combat numbers (Attune resonance,
/// retaliation, etc.) so the keyword suite can assert aura derivation without
/// exposing the internal CombatStats type.
pub struct CombatStatsView {
    pub attack: i16,
    pub defense: i16,
    pub resonance: Option<crate::types::Resonance>,
    pub retaliation: i16,
    pub dmg_reduction: i16,
    pub momentum_first_bonus: bool,
    pub chain_while_defeating: bool,
    pub momentum_per_link_bonus: i16,
}
