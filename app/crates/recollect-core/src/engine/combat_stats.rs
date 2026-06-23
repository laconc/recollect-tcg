//! Derived combat stats: the aura/buff fold + the test-visible derivation hooks.
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

/// Test hook: the arrival-targeting reach of the spirit at `tile` (its own card's
/// reach, widened by any reach aura), for `seat`'s facing.
pub fn targeting_reach_for_test(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    seat: Seat,
) -> Vec<u8> {
    let base = card(
        catalog,
        sim.spirit_at(tile).expect("a spirit stands here").card,
    )
    .reach;
    targeting_reach(sim, catalog, base, tile, seat, sim.board_w)
}

/// Test hook: does the spirit at `tile` have `kw` (intrinsic or aura-granted)?
pub fn keyword_active_for_test(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    kw: crate::effects::Keyword,
) -> bool {
    keyword_active(sim, catalog, tile, kw)
}

pub fn combat_stats_for_test(sim: &GameState, catalog: &[CardDef], tile: u8) -> CombatStatsView {
    let cs = combat_stats(sim, catalog, tile, None);
    CombatStatsView {
        attack: cs.atk,
        defense: cs.def,
        resonance: cs.resonance,
        retaliation: cs.retaliation,
        dmg_reduction: cs.dmg_reduction,
        momentum_first_bonus: cs.momentum_first_bonus,
        chain_while_defeating: cs.chain_while_defeating,
        momentum_per_link_bonus: cs.momentum_per_link_bonus,
    }
}

pub(crate) fn combat_stats(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    vs: Option<u8>,
) -> CombatStats {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let Some(me) = sim.spirit_at(tile) else {
        return CombatStats::default();
    };
    let mut cs = CombatStats {
        atk: me.attack,
        def: me.defense,
        resonance: Some(card(catalog, me.card).resonance),
        ..Default::default()
    };
    for m in &sim.temp_mods {
        if m.tile == tile {
            cs.atk += m.attack;
            cs.def += m.defense;
        }
    }
    // Reckless Charge: this-round retaliation shifts on this tile.
    for &(t, delta, until) in &sim.temp_retaliation {
        if t == tile && until >= sim.round {
            cs.retaliation += delta;
        }
    }
    let standing: Vec<(u8, &Spirit)> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit
                .as_ref()
                .filter(|s| !s.fading)
                .map(|s| (i as u8, s))
        })
        .collect();
    cs_scan_4(&mut cs, sim, catalog, me, &standing, tile, vs);
    // Landmark auras: the occupant gains the terrain's static clauses.
    if let Some(terr) = &sim.board[tile as usize].terrain
        && !terr.face_down
        && let Some(specs) = crate::effects::specs_for(catalog, terr.card)
    {
        for spec in specs {
            if spec.trigger != crate::effects::Trigger::Static {
                continue;
            }
            for cl in &spec.clauses {
                // OccupantHere buffs whoever stands on the terrain (most
                // landmarks are neutral ground); OccupantAndAdjacentAllies is
                // ally-only ("allies here and adjacent", The Trellis).
                let occ_applies = match cl.selector {
                    crate::effects::Selector::OccupantHere => true,
                    crate::effects::Selector::OccupantAndAdjacentAllies => me.owner == terr.owner,
                    _ => false,
                };
                if occ_applies {
                    if let crate::effects::Effect::StatDelta {
                        attack, defense, ..
                    } = cl.effect
                    {
                        cs.atk += attack;
                        cs.def += defense;
                    }
                    if let crate::effects::Effect::RetaliationDelta { amount } = cl.effect {
                        cs.retaliation += amount;
                    }
                }
            }
        }
    }
    // Bond auras: a bonded, adjacent pair shares its Bond's static clauses.
    cs_d_5_bond_auras_a_bonded(&mut cs, sim, catalog, me, tile);
    // Rondel, the Joining: a STANDING spirit (not a bond card) grants the owner's
    // bonded pairs the higher Defense/Attack — a seat-wide aura. Apply to `me` if it's
    // in an adjacent pair.
    let (rondel_atk, rondel_def) = owner_grants_pair_share(sim, catalog, me.owner);
    if rondel_atk || rondel_def {
        for b in &sim.bonds {
            let other = if b.tile_a == tile {
                Some(b.tile_b)
            } else if b.tile_b == tile {
                Some(b.tile_a)
            } else {
                None
            };
            if let Some(other) = other
                && manhattan(b.tile_a, b.tile_b) == 1
                && let Some(o) = sim.spirit_at(other).filter(|s| !s.fading)
            {
                if rondel_atk {
                    cs.atk = cs.atk.max(o.attack);
                }
                if rondel_def {
                    cs.def = cs.def.max(o.defense);
                }
            }
        }
    }
    // Duet Ascendant, Both Halves: a standing aura grants every one of the owner's bonded
    // spirits a FLAT extra stat (vs Rondel's higher-of-the-two share).
    let (duet_a, duet_d) = owner_grants_bond_stat(sim, catalog, me.owner);
    if (duet_a != 0 || duet_d != 0)
        && sim.bonds.iter().any(|b| {
            (b.tile_a == tile || b.tile_b == tile)
                && manhattan(b.tile_a, b.tile_b) == 1
                && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
                && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
        })
    {
        cs.atk += duet_a;
        cs.def += duet_d;
    }
    // Maestra Vole: an adjacent ally whose aura GRANTS Chorus (AdjacentAlliesAll/CounterAura)
    // gives this spirit a Chorus bonus, counting its own adjacent allies (capped at the grant).
    let granted_chorus = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let i = i as u8;
            if i == tile || manhattan(i, tile) != 1 {
                return None;
            }
            let sp = t.spirit.as_ref()?;
            if sp.owner != me.owner || sp.fading {
                return None;
            }
            crate::effects::specs_for(catalog, sp.card)?
                .iter()
                .filter(|s| s.trigger == Trigger::Static)
                .flat_map(|s| &s.clauses)
                .find_map(|cl| match (&cl.selector, &cl.effect) {
                    (S::AdjacentAlliesAll, E::CounterAura { max, .. }) => Some(*max),
                    _ => None,
                })
        })
        .max();
    // The Smudge / Null Choir blur an adjacent enemy's traits — the Chorus bonus is silenced.
    if let Some(gmax) = granted_chorus.filter(|_| !trait_silenced(sim, catalog, tile)) {
        let adj = standing
            .iter()
            .filter(|(i, o)| *i != tile && o.owner == me.owner && manhattan(*i, tile) == 1)
            .count() as i16;
        cs.atk += 10 * adj.min(gmax as i16);
    }
    // MomentumMod: set the derived chain flags momentum_prefs reads — a spirit's own aura
    // (SelfSpirit: Brand-Bearer Macaque, The Long Coronation) or a seat-wide ally aura
    // (AlliesAll: Sparkfather Vermilion). Authored but previously never applied.
    cs_alliesall_sparkfather_v(&mut cs, sim, catalog, me, tile);
    // Grudge-Kept (ill intent): +Attack for each ENEMY impression on the board (a SelfSpirit aura).
    if let Some(specs) = crate::effects::specs_for(catalog, me.card) {
        for spec in specs.iter().filter(|s| s.trigger == Trigger::Static) {
            for cl in &spec.clauses {
                if cl.selector == S::SelfSpirit
                    && let E::AttackPerEnemyImpression { amount } = cl.effect
                {
                    let impressions = sim
                        .board
                        .iter()
                        .filter(|t| t.impressions.contains(&me.owner.other()))
                        .count() as i16;
                    cs.atk += amount * impressions;
                }
                // The Worst Version: its Attack becomes the highest among enemy spirits.
                if cl.selector == S::SelfSpirit && matches!(cl.effect, E::CopyHighestEnemyAttack) {
                    let highest = sim
                        .board
                        .iter()
                        .filter_map(|t| t.spirit.as_ref())
                        .filter(|s| s.owner != me.owner && !s.fading)
                        .map(|s| s.attack)
                        .max();
                    if let Some(h) = highest {
                        cs.atk = h;
                    }
                }
            }
        }
    }
    // Co-Conspirators-style bond debuffs: a Bond can reach OUTSIDE its pair to
    // weaken enemies adjacent to either endpoint (selector AdjacentEnemiesAll).
    // Computed for spirits NOT in the bond, so it lives apart from the pair loop.
    // Gated on PairAdjacent: both ends present, non-fading, and adjacent.
    cs_gated_on_pairadjacent_bo(&mut cs, sim, catalog, me, tile);
    // The Trellis-style terrain: OccupantAndAdjacentAllies also buffs allies
    // ADJACENT to the landmark (the "here" half is in the occupant loop above).
    // Scan neighboring tiles for terrain carrying that selector and credit
    // spirits allied to the terrain's owner.
    cs_spirits_allied_to_the_te(&mut cs, sim, catalog, me, tile);
    // Whisperer at the Door: when this spirit is the ATTACKER striking `vs`, a defensive
    // engage-aura on the target (SelfSpirit/EngagerAttackDelta) shifts the attacker's Attack.
    if let Some(v) = vs
        && let Some(d) = sim.spirit_at(v)
        && !d.fading
        && let Some(specs) = crate::effects::specs_for(catalog, d.card)
    {
        for s in specs.iter().filter(|s| s.trigger == Trigger::Static) {
            for cl in &s.clauses {
                if cl.selector == S::SelfSpirit
                    && let E::EngagerAttackDelta { attack } = cl.effect
                {
                    cs.atk += attack;
                }
            }
        }
    }
    cs
}

fn cs_spirits_allied_to_the_te(
    cs: &mut CombatStats,
    sim: &GameState,
    catalog: &[CardDef],
    me: &Spirit,
    tile: u8,
) {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    for (n, t) in sim.board.iter().enumerate() {
        if manhattan(tile, n as u8) != 1 {
            continue;
        }
        let Some(terr) = &t.terrain else { continue };
        if terr.face_down || me.owner != terr.owner {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, terr.card) {
            for spec in specs {
                // Static auras, plus OnReveal "becomes a Landmark" auras (The Long
                // Table) whose WhilePresent clauses stay on once the terrain is up.
                if spec.trigger != Trigger::Static && spec.trigger != Trigger::OnReveal {
                    continue;
                }
                for cl in &spec.clauses {
                    // The Trellis (OccupantAndAdjacentAllies) and The Long Table
                    // (AdjacentAlliesAll) both buff allies adjacent to the landmark.
                    if matches!(
                        cl.selector,
                        S::OccupantAndAdjacentAllies | S::AdjacentAlliesAll
                    ) && cl.duration == crate::effects::Duration::WhilePresent
                        && let E::StatDelta {
                            attack, defense, ..
                        } = cl.effect
                    {
                        cs.atk += attack;
                        cs.def += defense;
                    }
                }
            }
        }
    }
}

fn cs_gated_on_pairadjacent_bo(
    cs: &mut CombatStats,
    sim: &GameState,
    catalog: &[CardDef],
    me: &Spirit,
    tile: u8,
) {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    for b in &sim.bonds {
        if me.owner == b.owner {
            continue; // only enemies of the bond's owner are debuffed
        }
        if manhattan(b.tile_a, tile) != 1 && manhattan(b.tile_b, tile) != 1 {
            continue; // must be adjacent to one of the bonded pair
        }
        let pair_ok = manhattan(b.tile_a, b.tile_b) == 1
            && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
            && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false);
        if !pair_ok {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, b.card) {
            for spec in specs {
                if spec.trigger != Trigger::Static
                    || spec.condition != crate::effects::Condition::PairAdjacent
                {
                    continue;
                }
                for cl in &spec.clauses {
                    if cl.selector == S::AdjacentEnemiesAll
                        && let E::StatDelta {
                            attack, defense, ..
                        } = &cl.effect
                    {
                        cs.atk += attack;
                        cs.def += defense;
                    }
                }
            }
        }
    }
}

fn cs_alliesall_sparkfather_v(
    cs: &mut CombatStats,
    sim: &GameState,
    catalog: &[CardDef],
    me: &Spirit,
    tile: u8,
) {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    for (i, t) in sim.board.iter().enumerate() {
        let Some(ssp) = &t.spirit else { continue };
        if ssp.fading || ssp.face_down {
            continue;
        }
        let is_self = i as u8 == tile;
        let is_ally = ssp.owner == me.owner;
        if !is_self && !is_ally {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, ssp.card) {
            for spec in specs.iter().filter(|s| s.trigger == Trigger::Static) {
                for cl in &spec.clauses {
                    if let E::MomentumMod {
                        first_engage_bonus,
                        chain_while_defeating,
                        per_link_bonus,
                        chain_no_retaliation,
                    } = cl.effect
                    {
                        let applies = (cl.selector == S::SelfSpirit && is_self)
                            || (cl.selector == S::AlliesAll && is_ally);
                        if applies {
                            cs.momentum_first_bonus |= first_engage_bonus;
                            cs.chain_while_defeating |= chain_while_defeating;
                            cs.momentum_per_link_bonus += per_link_bonus;
                            cs.chain_no_retaliation |= chain_no_retaliation;
                        }
                    }
                }
            }
        }
    }
}

fn cs_d_5_bond_auras_a_bonded(
    cs: &mut CombatStats,
    sim: &GameState,
    catalog: &[CardDef],
    me: &Spirit,
    tile: u8,
) {
    for b in &sim.bonds {
        let in_pair = b.tile_a == tile || b.tile_b == tile;
        if !in_pair {
            continue;
        }
        let adjacent = manhattan(b.tile_a, b.tile_b) == 1
            && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
            && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false);
        if !adjacent {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, b.card) {
            for spec in specs {
                if spec.trigger != crate::effects::Trigger::Static {
                    continue;
                }
                for cl in &spec.clauses {
                    if cl.selector != crate::effects::Selector::BondedPair {
                        continue;
                    }
                    match &cl.effect {
                        crate::effects::Effect::StatDelta {
                            attack, defense, ..
                        } => {
                            // WhileDamaged on a bond means BOTH ends damaged
                            // (Rivals' Pact: "+10 Attack each while both damaged").
                            let gated = match spec.condition {
                                crate::effects::Condition::WhileDamaged => {
                                    let other = if b.tile_a == tile { b.tile_b } else { b.tile_a };
                                    me.hp < me.hp_max
                                        && sim
                                            .spirit_at(other)
                                            .map(|o| o.hp < o.hp_max)
                                            .unwrap_or(false)
                                }
                                _ => true,
                            };
                            if gated {
                                cs.atk += attack;
                                cs.def += defense;
                            }
                        }
                        crate::effects::Effect::StatShareHigher { attack, defense } => {
                            let other = if b.tile_a == tile { b.tile_b } else { b.tile_a };
                            if let Some(o) = sim.spirit_at(other) {
                                if *attack {
                                    cs.atk = cs.atk.max(o.attack);
                                }
                                if *defense {
                                    cs.def = cs.def.max(o.defense);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn cs_scan_4(
    cs: &mut CombatStats,
    sim: &GameState,
    catalog: &[CardDef],
    me: &Spirit,
    standing: &[(u8, &Spirit)],
    tile: u8,
    vs: Option<u8>,
) {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    for (src, ssp) in standing {
        let Some(specs) = crate::effects::specs_for(catalog, ssp.card) else {
            continue;
        };
        for spec in specs {
            if spec.trigger != Trigger::Static || !condition_holds(sim, &spec.condition, *src, ssp)
            {
                continue;
            }
            for cl in &spec.clauses {
                let applies = match &cl.selector {
                    S::SelfSpirit => *src == tile,
                    S::AdjacentAlliesAll => {
                        ssp.owner == me.owner && *src != tile && manhattan(*src, tile) == 1
                    }
                    S::AlliesAll => ssp.owner == me.owner,
                    S::AlliesInReach => {
                        ssp.owner == me.owner
                            && *src != tile
                            && oriented(card(catalog, ssp.card).reach, *src, ssp.owner)
                                .contains(&tile)
                    }
                    S::AlliesWithImprint { imprint } => {
                        ssp.owner == me.owner
                            && !me.traits_blanked(sim.round)
                            && card(catalog, me.card).imprints.iter().any(|i| i == imprint)
                    }
                    S::EnemiesWithinTiles { tiles } => {
                        ssp.owner != me.owner && manhattan(*src, tile) <= *tiles as i16
                    }
                    S::AdjacentEnemiesAll => {
                        ssp.owner != me.owner && *src != tile && manhattan(*src, tile) == 1
                    }
                    S::EnemiesEngagingAdjacentAllies => {
                        ssp.owner != me.owner
                            && vs
                                .map(|v| {
                                    manhattan(*src, v) == 1
                                        && sim
                                            .spirit_at(v)
                                            .map(|d| d.owner == ssp.owner)
                                            .unwrap_or(false)
                                })
                                .unwrap_or(false)
                    }
                    S::EnemiesAll => ssp.owner != me.owner,
                    _ => false,
                };
                if !applies {
                    continue;
                }
                match &cl.effect {
                    E::StatDelta {
                        attack, defense, ..
                    } => {
                        cs.atk += attack;
                        cs.def += defense;
                    }
                    E::RetaliationDelta { amount } if *src == tile => cs.retaliation += amount,
                    E::DamageReduction { amount } => cs.dmg_reduction += amount,
                    E::CounterAura { max, .. } if *src == tile => {
                        // Chorus counts adjacent allies; an Unbreakable-bonded partner
                        // counts as adjacent across its 1-tile gap. Silenced to nothing
                        // while adjacent to an enemy TraitSilence (The Smudge / Null Choir).
                        let adj = if trait_silenced(sim, catalog, tile) {
                            0
                        } else {
                            standing
                                .iter()
                                .filter(|(i, o)| {
                                    *i != tile
                                        && o.owner == me.owner
                                        && (manhattan(*i, tile) == 1
                                            || unbreakable_bridges(sim, catalog, tile, *i))
                                })
                                .count() as i16
                        };
                        cs.atk += 10 * adj.min(*max as i16);
                    }
                    E::GrantSharedResonance if *src == tile => {
                        if let Some(r) = shared_adjacent_resonance(sim, catalog, tile, me, 2) {
                            cs.resonance = Some(r);
                        }
                    }
                    E::EdgeNegate => cs.edge_negated_against = true,

                    _ => {}
                }
            }
        }
    }
}
