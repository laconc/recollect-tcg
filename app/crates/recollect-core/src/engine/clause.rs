//! Clause application: banish/replace, apply_clause_at, choice effects, push_away.
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

/// The replaced banishment was never journaled. Returns true if the
/// spirit actually fades (no replacement applied).
pub(crate) fn banish_or_replace(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    tile: u8,
    by: Seat,
) -> bool {
    use crate::effects::{Condition, Effect, Replacement, Trigger};
    if let Some(sp) = sim.spirit_at(tile)
        && !sp.replacement_used
    {
        let name = card(catalog, sp.card).name.clone();
        if let Some(specs) = crate::effects::canon_effects()
            .specs
            .get(crate::cards::key_of(&name))
        {
            for spec in specs {
                if spec.trigger == Trigger::Static && spec.condition == Condition::OncePerMatch {
                    for cl in &spec.clauses {
                        if let Effect::Replace(Replacement::SurviveBanishAt { form }) = cl.effect {
                            push(sim, evs, Event::ReplacementSurvived { tile, form });
                            return false;
                        }
                    }
                }
            }
        }
    }
    // A defeated KINDRED (a `CardKind::Kindred` summon — NOT an Unwritten) leaves no
    // impression (§10: "dissolve to no impression") and cannot evolve, so the
    // standing-Faded window has no purpose for it: dissolve it AT ONCE, with no mark.
    // Recording `banished_by` on it would (a) violate "a token never records a
    // banisher" (invariants.rs #5) and (b) lay the banisher's impression at its
    // turn-END `dissolve_faded_at`, both wrong. An UNWRITTEN token is excepted — it
    // CAN be Primal-Deepened from this state (§5), so it enters the window like any
    // base (its own dissolve already leaves nothing). Parting still fires (§3: "Parting
    // triggers on every full dissolve"), then the broadcast, then a no-mark dissolve.
    if let Some(sp) = sim.spirit_at(tile)
        && sp.is_token
        && !card(catalog, sp.card).is_unwritten()
    {
        let (name, owner) = (card(catalog, sp.card).name.clone(), sp.owner);
        fire_doctrine(
            sim,
            evs,
            catalog,
            &name,
            Trigger::Parting,
            Some(tile),
            owner,
        );
        // Remove the token leaving NOTHING (no impression — the irreplaceable rule).
        if sim.spirit_at(tile).is_some() {
            push(sim, evs, Event::TokenDissolved { tile });
        }
        dissolution_effects(sim, evs, catalog, None);
        return true;
    }
    push(
        sim,
        evs,
        Event::SpiritBecameFading {
            tile,
            banished_by: Some(by),
        },
    );
    // OnAllyFading: every standing ally of the fading spirit witnesses it.
    let owner = sim.spirit_at(tile).map(|s| s.owner);
    if let Some(owner) = owner {
        let allies: Vec<(u8, CardId)> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.spirit
                    .as_ref()
                    .filter(|s| !s.fading && s.owner == owner && i as u8 != tile)
                    .map(|s| (i as u8, s.card))
            })
            .collect();
        for (atile, cid) in allies {
            let name = card(catalog, cid).name.clone();
            fire_effects_noctx(
                sim,
                evs,
                catalog,
                &name,
                Trigger::OnAllyFading,
                Some(atile),
                owner,
            );
        }
    }
    // The final-round refinement (design §0.5). On round 12 there is no next
    // owner turn to host a Primal-evolve window — but the spirit still **lingers
    // standing-Faded** through the rest of the round rather than vanishing the
    // instant it is defeated. It carries the same `fade_deadline` (stamped in
    // `evolve` for any combat banish) and stands Fading until the **end of round
    // 12**, when the Nightfall `finish` step dissolves every remaining fading
    // spirit BEFORE scoring — laying the banisher's impression so the OPPONENT
    // scores the tile. (Dissolving on defeat would rob the round of a body that
    // should stand until the telling ends; dissolving *after* scoring would
    // wrongly let the faded spirit, not the banisher's impression, hold the tile.)
    // So there is no immediate dissolve here — the round-12 base lingers like any
    // other banished base, just with no Main left to be evolved in.
    let _ = by;
    true
}

/// Apply one clause's effect at a doctrine-chosen tile (Parting path).
pub(crate) fn apply_clause_at(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    t: u8,
) {
    use crate::effects::{Duration, Effect as E};
    match &cl.effect {
        E::RestoreForm { amount } => push(
            sim,
            evs,
            Event::EffectRestored {
                tile: t,
                amount: *amount,
            },
        ),
        E::StatDelta {
            attack,
            defense,
            form,
        } => {
            if cl.duration == Duration::ThisRound {
                let until = sim.round;
                push(
                    sim,
                    evs,
                    Event::EffectTempStat {
                        tile: t,
                        attack: *attack,
                        defense: *defense,
                        until_round: until,
                    },
                );
            } else {
                push(
                    sim,
                    evs,
                    Event::EffectStat {
                        tile: t,
                        attack: *attack,
                        defense: *defense,
                        form: *form,
                    },
                );
            }
        }
        // The Solace's doctrine-resolved free-target effects (What You Set Down bounces,
        // The Kind Erasure releases, The Unsaid Cruelty damages) — guarded on a spirit present.
        E::Bounce if sim.spirit_at(t).is_some() => push(sim, evs, Event::SpiritBounced { tile: t }),
        // Release (mercy: the dying) and Banish (erasure: the living too) both
        // remove a present spirit leaving no impression — the same per-tile shape.
        E::Release | E::Banish if sim.spirit_at(t).is_some() => {
            push(sim, evs, Event::SpiritReleased { tile: t })
        }
        E::Damage { amount } if sim.spirit_at(t).is_some() => push(
            sim,
            evs,
            Event::EffectDamaged {
                tile: t,
                amount: *amount,
            },
        ),
        _ => {}
    }
}

/// Whether the spirit at `tile` has keyword `kw` — intrinsic on its card, OR
/// granted by a Static aura. Movement/push keywords (Mobile/Steadfast) consult
/// auras; combat keywords (Warded/Arcane) still read the card directly (their
/// read-sites thread through the pure forecast and aren't aura-aware yet).
/// Effective Warded of the DEFENDER at `tile` — intrinsic OR aura-granted (Oathbound's
/// bonded pair, a Warded-granting Landmark). Routed only on the defender side of each
/// exchange (standing board spirits with stable auras), keeping forecast/actual parity.
pub(crate) fn eff_warded(sim: &GameState, catalog: &[CardDef], tile: u8) -> bool {
    keyword_active(sim, catalog, tile, crate::effects::Keyword::Warded)
}

pub(crate) fn keyword_active(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    kw: crate::effects::Keyword,
) -> bool {
    use crate::effects::Keyword as K;
    let Some(sp) = sim.spirit_at(tile) else {
        return false;
    };
    let def = card(catalog, sp.card);
    // The Blank Page (permanent) / Smear (this round) strip PRINTED keywords (aura-granted
    // ones still apply below).
    let intrinsic = if sp.traits_blanked(sim.round) {
        false
    } else {
        match kw {
            K::Mobile => def.mobile,
            K::Steadfast => def.steadfast,
            K::Warded => def.warded,
            K::Arcane => def.arcane,
            K::Relentless => def.relentless,
            K::Lurk => false,
        }
    };
    // Played-card grants (Dig In, The Long Watch) ride on the spirit and, like
    // aura grants, are NOT silenced by The Blank Page.
    let played_grant = sp
        .kw_grants
        .iter()
        .any(|(k, until)| *k == kw && *until >= sim.round);
    intrinsic || played_grant || granted_keyword(sim, catalog, tile, kw)
}

/// Static keyword grants reaching `tile`: a bonded pair (BondedPair/GrantKeyword,
/// while the pair is present & adjacent) or the Landmark it stands on
/// (OccupantHere/GrantKeyword). Pure function of public board state.
pub(crate) fn granted_keyword(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    kw: crate::effects::Keyword,
) -> bool {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let clause_grants = |card_id: crate::types::CardId, want: S| -> bool {
        let Some(specs) = crate::effects::specs_for(catalog, card_id) else {
            return false;
        };
        specs.iter().any(|spec| {
            spec.trigger == Trigger::Static
                && spec.clauses.iter().any(|cl| {
                    cl.selector == want
                        && matches!(&cl.effect, E::GrantKeyword { keyword } if *keyword == kw)
                })
        })
    };
    for b in &sim.bonds {
        if b.tile_a != tile && b.tile_b != tile {
            continue;
        }
        let adjacent = manhattan(b.tile_a, b.tile_b) == 1
            && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
            && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false);
        if adjacent && clause_grants(b.card, S::BondedPair) {
            return true;
        }
    }
    if let Some(terr) = &sim.board[tile as usize].terrain
        && !terr.face_down
        && clause_grants(terr.card, S::OccupantHere)
    {
        return true;
    }
    // Oathkeeper Adamant: an adjacent ally's standing aura grants this keyword.
    let me_owner = sim.spirit_at(tile).map(|s| s.owner);
    if let Some(owner) = me_owner {
        for (i, t) in sim.board.iter().enumerate() {
            if i as u8 != tile
                && manhattan(i as u8, tile) == 1
                && let Some(sp) = &t.spirit
                && !sp.fading
                && sp.owner == owner
                && clause_grants(sp.card, S::AdjacentAlliesAll)
            {
                return true;
            }
        }
    }
    false
}

/// Apply a resolved Target choice as concrete facts.
pub(crate) fn apply_choice_effect(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    effect: crate::state::ChoiceEffect,
    source: u8,
    tile: u8,
    owner: Seat,
) {
    use crate::state::ChoiceEffect as CE;
    // A separate Arc clone so catalog-reading calls and the `&mut ctx` engage path
    // (EngageFrom → full_exchange) don't conflict on the borrow.
    let catalog = &ctx.catalog.clone();
    match effect {
        CE::PushAway { tiles } => push_away(sim, evs, catalog, source, tile, tiles, owner),
        CE::RestoreForm { amount } => push(sim, evs, Event::EffectRestored { tile, amount }),
        CE::Damage { .. } => choice_damage(sim, evs, ctx, effect, tile, owner),
        CE::RemoveImpression => {
            // Unwriting: the Solace erasing an EXISTING mark scores (the same as forgetting); a
            // Lorekeeper's removal just clears it, and an empty target erases nothing, so no tally.
            let solace = sim.rules.factions[owner as usize] == crate::types::Faction::Solace;
            if solace && !sim.board[tile as usize].impressions.is_empty() {
                push(sim, evs, Event::ImpressionForgotten { tile });
            } else {
                push(sim, evs, Event::ImpressionUnwritten { tile });
            }
        }
        CE::GrantKeyword {
            keyword,
            until_round,
        } => push(
            sim,
            evs,
            Event::KeywordGranted {
                tile,
                keyword,
                until_round,
            },
        ),
        CE::DisplaceFrom { .. } => choice_displace_from(sim, evs, effect, tile, owner),
        CE::MoveTo { from } => {
            // Step 2 resolved: `tile` is the destination. Slide the spirit over — only
            // onto an empty, terrain-free tile (a spirit never shares a tile with
            // terrain; the option list already filters these, this guards the apply).
            if sim.spirit_at(from).is_some()
                && sim.spirit_at(tile).is_none()
                && sim.board[tile as usize].terrain.is_none()
            {
                push(sim, evs, Event::SpiritPushed { from, to: tile });
            }
        }
        CE::PairBuffStep1 { .. } => choice_pair_buff_step1(sim, evs, effect, tile, owner),
        CE::PairBuffWith { .. } => choice_pair_buff_with(sim, evs, effect, tile),
        CE::SwapStep1 => choice_swap_step1(sim, evs, tile, owner),
        CE::SwapWith { from } => {
            // Step 2 resolved: exchange the two spirits.
            if sim.spirit_at(from).is_some() && sim.spirit_at(tile).is_some() {
                push(sim, evs, Event::SpiritsSwapped { a: from, b: tile });
            }
        }
        CE::HealBondedPair { .. } => choice_heal_bonded_pair(sim, evs, effect, tile),
        CE::Bounce => {
            if sim.spirit_at(tile).is_some() {
                push(sim, evs, Event::SpiritBounced { tile });
            }
        }
        CE::RevealFabricationAt => choice_reveal_fabrication_at(sim, evs, ctx, tile),
        CE::ReachBuff { .. } => choice_reach_buff(sim, evs, effect, tile, owner),
        CE::EngageFrom { .. } => choice_engage_from(sim, evs, ctx, effect, tile),
        CE::EngageStep1 { .. } => choice_engage_step1(sim, evs, ctx, effect, tile, owner),
        CE::RetaliateThisRound { .. } => choice_retaliate_this_round(sim, evs, effect, tile),
        CE::PayEngage { .. } => choice_pay_engage(sim, evs, ctx, effect, tile),
        CE::PeekFabrication => choice_peek_fabrication(sim, evs, tile, owner),
        CE::ReTriggerParting => choice_re_trigger_parting(sim, evs, ctx, tile, owner),
        CE::RestrictEngage { until_round } => {
            if sim.spirit_at(tile).is_some() {
                push(sim, evs, Event::EngageRestricted { tile, until_round });
            }
        }
        CE::DelayFade => {
            if sim.spirit_at(tile).map(|s| s.fading).unwrap_or(false) {
                push(sim, evs, Event::FadeDelayed { tile });
            }
        }
        CE::StatDelta { .. } => choice_stat_delta(sim, evs, effect, tile),
    }
}

/// Push the spirit at `tile` one step directly away from `source`; a
/// blocked, faded, or off-board step simply doesn't move.
/// The Threshold: does the (revealed) terrain on `tile` grant its occupant a
/// `Restrict` (here BePushed) via a Static OccupantHere clause? A per-tile aura,
/// consulted at the push chokepoint alongside Steadfast and the seat-wide
/// Stand Ground restriction.
pub(crate) fn occupant_restricted(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    r: crate::effects::Restriction,
) -> bool {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let Some(terr) = sim.board[tile as usize].terrain.as_ref() else {
        return false;
    };
    if terr.face_down {
        return false;
    }
    crate::effects::specs_for(catalog, terr.card)
        .map(|specs| {
            specs.iter().any(|s| {
                s.trigger == Trigger::Static
                    && s.clauses.iter().any(|cl| {
                        cl.selector == S::OccupantHere
                            && matches!(&cl.effect, E::Restrict(rr) if *rr == r)
                    })
            })
        })
        .unwrap_or(false)
}

/// Common Ground: does the (revealed) terrain on `tile`, owned by `seat`, carry a
/// Static OccupantHere clause declaring `which` exception? A POSITIONAL exception
/// (read by tile), distinct from the seat-wide `exception_active`.
pub(crate) fn tile_terrain_exception(
    sim: &GameState,
    catalog: &[CardDef],
    tile: u8,
    seat: Seat,
    which: crate::effects::RuleException,
) -> bool {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let Some(terr) = sim.board[tile as usize].terrain.as_ref() else {
        return false;
    };
    if terr.face_down || terr.owner != seat {
        return false;
    }
    crate::effects::specs_for(catalog, terr.card)
        .map(|specs| {
            specs.iter().any(|s| {
                s.trigger == Trigger::Static
                    && s.clauses.iter().any(|cl| {
                        cl.selector == S::OccupantHere
                            && matches!(&cl.effect, E::Exception(x) if *x == which)
                    })
            })
        })
        .unwrap_or(false)
}

pub(crate) fn push_away(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    source: u8,
    tile: u8,
    _tiles: u8,
    pusher: Seat,
) {
    // Steadfast: cannot be forcibly displaced — the push simply fails (design
    // Steadfast: "pushes, pulls, and swaps fail"). Aura-granted Steadfast
    // (Shoulder to Shoulder) counts too. Check before moving.
    let Some(owner) = sim.spirit_at(tile).map(|sp| sp.owner) else {
        return;
    };
    if keyword_active(sim, catalog, tile, crate::effects::Keyword::Steadfast) {
        return;
    }
    // Stand Ground: a this-round seat-wide BePushed restriction also blocks it.
    if restricted(sim, owner, crate::effects::Restriction::BePushed) {
        return;
    }
    // The Threshold: a Landmark whose occupant cannot be pushed (per-tile).
    if occupant_restricted(sim, catalog, tile, crate::effects::Restriction::BePushed) {
        return;
    }
    // Arbiter Imperishable: a standing aura makes the owner's spirits unpushable.
    if seat_grants_push_immunity(sim, catalog, owner) {
        return;
    }
    let w = sim.board_w as i16;
    let (sx, sy) = (source as i16 % w, source as i16 / w);
    let (tx, ty) = (tile as i16 % w, tile as i16 / w);
    let dx = (tx - sx).signum();
    let dy = (ty - sy).signum();
    let (nx, ny) = (tx + dx, ty + dy);
    if !(0..w).contains(&nx) || !(0..w).contains(&ny) {
        return;
    }
    let dest = (ny * w + nx) as u8;
    // A spirit can never share a tile with terrain (invariant #1) or a Stray (1b), so a
    // push lands only on an open tile (unfaded, spirit-free, terrain-free, no Stray) — a
    // Landmark, a revealed Fabrication, or the wild blocks the landing; the shove fails.
    let ok = tile_open_for_arrival(sim, dest);
    if ok {
        push(
            sim,
            evs,
            Event::SpiritPushed {
                from: tile,
                to: dest,
            },
        );
        let _ = pusher;
        // Herald of the Solace: an enemy shoved onto an Impression-bearing tile takes damage.
        if !sim.board[dest as usize].impressions.is_empty()
            && let Some(dmg) = impression_push_damage(sim, catalog, owner)
        {
            push(
                sim,
                evs,
                Event::EffectDamaged {
                    tile: dest,
                    amount: dmg,
                },
            );
            if sim
                .spirit_at(dest)
                .map(|s| s.hp <= 0 && !s.fading)
                .unwrap_or(false)
            {
                banish_or_replace(sim, evs, catalog, dest, owner.other());
            }
        }
    }
}

/// Herald of the Solace: if the spirit at the pushed tile is an enemy of a standing Herald,
/// the damage that Herald's ImpressionPushDamage aura deals (the pushed spirit's owner is `owner`).
/// Badgermarshal: the chain-damage reduction the defender at `tile` receives from an allied
/// neighbour carrying AdjacentAlliesAll/ChainDamageReductionAura.
pub(crate) fn chain_damage_reduction(sim: &GameState, catalog: &[CardDef], tile: u8) -> i16 {
    let Some(me) = sim.spirit_at(tile) else {
        return 0;
    };
    let mut red = 0;
    for (i, t) in sim.board.iter().enumerate() {
        let i = i as u8;
        if i == tile || manhattan(i, tile) != 1 {
            continue;
        }
        let Some(sp) = &t.spirit else { continue };
        if sp.owner != me.owner || sp.fading {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, sp.card) {
            for spec in specs
                .iter()
                .filter(|s| s.trigger == crate::effects::Trigger::Static)
            {
                for cl in &spec.clauses {
                    if cl.selector == crate::effects::Selector::AdjacentAlliesAll
                        && let crate::effects::Effect::ChainDamageReductionAura { amount } =
                            cl.effect
                    {
                        red += amount;
                    }
                }
            }
        }
    }
    red
}

pub(crate) fn impression_push_damage(
    sim: &GameState,
    catalog: &[CardDef],
    owner: Seat,
) -> Option<i16> {
    let foe = owner.other();
    let mut dmg = 0i16;
    for t in &sim.board {
        let Some(sp) = &t.spirit else { continue };
        if sp.owner != foe || sp.fading || sp.face_down {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, sp.card) {
            for s in specs
                .iter()
                .filter(|s| s.trigger == crate::effects::Trigger::Static)
            {
                for cl in &s.clauses {
                    if let crate::effects::Effect::ImpressionPushDamage { amount } = cl.effect {
                        dmg += amount;
                    }
                }
            }
        }
    }
    (dmg > 0).then_some(dmg)
}

/// Parting doctrine: the dying don't deliberate. Restores find the most
/// wounded eligible; buffs the highest Attack; pushes flee the teller.
pub(crate) fn doctrine_pick(
    sim: &GameState,
    sel: &crate::effects::Selector,
    eff: &crate::effects::Effect,
    source: u8,
    owner: Seat,
) -> Option<u8> {
    use crate::effects::{Effect as E, Selector as S};
    let candidates: Vec<u8> = match sel {
        S::AdjacentAllyChoose => sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.spirit
                    .as_ref()
                    .filter(|sp| !sp.fading && sp.owner == owner && manhattan(source, i as u8) == 1)
                    .map(|_| i as u8)
            })
            .collect(),
        S::AdjacentEnemyChoose => sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.spirit
                    .as_ref()
                    .filter(|sp| !sp.fading && sp.owner != owner && manhattan(source, i as u8) == 1)
                    .map(|_| i as u8)
            })
            .collect(),
        _ => return None,
    };
    match eff {
        E::RestoreForm { .. } => candidates
            .into_iter()
            .min_by_key(|t| sim.spirit_at(*t).map(|s| s.hp - s.hp_max).unwrap_or(0)),
        _ => candidates
            .into_iter()
            .max_by_key(|t| sim.spirit_at(*t).map(|s| s.attack).unwrap_or(0)),
    }
}

/// OnReveal effects after a FORCED reveal (engaged while lurking).
pub(crate) fn forced_reveal_effects(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    tile: u8,
) {
    if let Some(sp) = sim.spirit_at(tile) {
        let (name, owner) = (card(catalog, sp.card).name.clone(), sp.owner);
        fire_effects_noctx(
            sim,
            evs,
            catalog,
            &name,
            crate::effects::Trigger::OnReveal,
            Some(tile),
            owner,
        );
    }
}

/// Tokens fade (no impression) when no living caller of theirs remains. The
/// Solace's Unwritten are exempt — they are standalone creatures (no caller), not
/// Kindred summons, and persist until banished (they are the PvE faction's bodies).
pub(crate) fn sweep_orphan_tokens(sim: &mut GameState, evs: &mut Vec<Event>, catalog: &[CardDef]) {
    let callers_alive: Vec<(Seat, String)> = sim
        .board
        .iter()
        .filter_map(|t| t.spirit.as_ref().filter(|s| !s.fading))
        .filter_map(|s| {
            crate::effects::specs_for(catalog, s.card).and_then(|specs| {
                specs.iter().flat_map(|sp| &sp.clauses).find_map(|cl| {
                    if let crate::effects::Effect::Summon { card_name } = &cl.effect {
                        Some((s.owner, card_name.clone()))
                    } else {
                        None
                    }
                })
            })
        })
        .collect();
    let orphans: Vec<u8> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit
                .as_ref()
                // Unwritten are standalone PvE creatures, never orphan-swept.
                .filter(|s| s.is_token && !s.fading && !card(catalog, s.card).is_unwritten())
                .map(|s| (i as u8, s.owner, card(catalog, s.card).name.clone()))
        })
        .filter(|(_, owner, name)| !callers_alive.iter().any(|(o, n)| o == owner && n == name))
        .map(|(i, _, _)| i)
        .collect();
    for tile in orphans {
        push(sim, evs, Event::TokenDissolved { tile });
    }
}
/// Manifest `card_name` on the lowest-index adjacent empty tile, one
/// Kindred per caller at a time.
pub(crate) fn summon_kindred(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    source: Option<u8>,
    owner: Seat,
) {
    let Some(src) = source else { return };
    let already = sim.board.iter().any(|t| {
        t.spirit
            .as_ref()
            .map(|sp| {
                sp.is_token
                    && sp.owner == owner
                    && card(catalog, sp.card).name == card_name
                    && !sp.fading
            })
            .unwrap_or(false)
    });
    if already {
        return;
    }
    if let Some(tok) = catalog.iter().find(|c| c.name == card_name)
        && let Some(dest) = crate::types::adjacent4(src)
            // A Kindred lands on an OPEN tile — unfaded, spirit-free, terrain-free, and not
            // a Stray's (a spirit never coexists with a Landmark/Fabrication, inv #1, nor
            // the wild, 1b).
            .find(|&a| tile_open_for_arrival(sim, a))
    {
        push(
            sim,
            evs,
            Event::SpiritManifested {
                seat: owner,
                card: tok.id,
                tile: dest,
                attack: tok.attack,
                defense: tok.defense,
                hp: tok.hp,
            },
        );
    }
}
/// OnEngageResolved with the Survivor selector — the still-standing
/// party of the source's engage. If the defender fell, the survivor is the
/// attacker; otherwise the owner picks deterministically (the loser, if any).
pub(crate) fn fire_survivor(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    source: u8,
    owner: Seat,
    att_tile: u8,
    def_tile: u8,
    defender_banished: bool,
) {
    use crate::effects::{Condition, Effect, Selector, Trigger};
    let Some(specs) = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(card_name))
    else {
        return;
    };
    let specs = specs.clone();
    for spec in &specs {
        if spec.trigger != Trigger::OnEngageResolved || spec.condition != Condition::Always {
            continue;
        }
        for cl in &spec.clauses {
            // The Misremembered (attacking): SelfSpirit/CopyPrintedStats — copy the printed
            // A/D of the DEFENDER it just fought. (The defender-side direction is bound by
            // `fire_engaged`; this is the symmetric attacker case.)
            if cl.selector == Selector::SelfSpirit && matches!(cl.effect, Effect::CopyPrintedStats)
            {
                exec_copy_printed_stats(sim, evs, catalog, Some(att_tile), Some(def_tile));
                continue;
            }
            if cl.selector != Selector::Survivor {
                continue;
            }
            // The survivor: defender if it stands, else the attacker.
            let survivor = if defender_banished {
                att_tile
            } else if sim.spirit_at(def_tile).map(|s| !s.fading).unwrap_or(false) {
                def_tile
            } else {
                att_tile
            };
            if let Effect::Displace(crate::effects::Displacement::Push { tiles }) = cl.effect {
                push_away(sim, evs, catalog, source, survivor, tiles, owner);
            }
        }
    }
}

/// The forms a Fading base may become — shared-Imprint rule, with the
/// authored RuleException carriers honored (Bearer of Small Stones this turn;
/// Duckling treats ALL Imprints as shared).
pub(crate) fn legal_evolutions(
    st: &GameState,
    catalog: &[CardDef],
    base: &CardDef,
    owner: Seat,
) -> Vec<CardId> {
    // Tier-gate (owner ruling): only a true BASE evolves. A Primal cannot jump
    // to Fabled. Forms have `evolves_from` set; bases do not.
    if base.evolves_from.is_some() {
        return Vec::new();
    }
    // The shared-Imprint rule is excepted by carriers of either
    // EvolveIgnoresSharedImprint (Bearer of Small Stones) or AllImprintsShared
    // (Duckling Following Anyone) — consulted through the general dispatch.
    use crate::effects::RuleException::{AllImprintsShared, EvolveIgnoresSharedImprint};
    let ignore_imprint = exception_active(st, catalog, owner, EvolveIgnoresSharedImprint)
        || exception_active(st, catalog, owner, AllImprintsShared)
        // Bearer of Small Stones: a Parting freed this turn's evolutions.
        || st.ignore_imprint_this_turn[owner as usize]
        // Shrine of the Nameless: a fading spirit resting on it fuels imprint-free evolution.
        || shrine_fading_fuel(st, catalog, owner);
    base.evolves_to
        .iter()
        .filter_map(|form_name| {
            let form = catalog.iter().find(|c| &c.name == form_name)?;
            // The form must share an Imprint with its base (unless excepted).
            let shares = ignore_imprint
                || form.imprints.is_empty()
                || base.imprints.is_empty()
                || form.imprints.iter().any(|i| base.imprints.contains(i));
            shares.then_some(form.id)
        })
        .collect()
}
#[doc(hidden)]
pub fn legal_evolutions_for_test(
    st: &crate::state::GameState,
    catalog: &[crate::types::CardDef],
    base: &crate::types::CardDef,
    owner: crate::types::Seat,
) -> Vec<crate::types::CardId> {
    legal_evolutions(st, catalog, base, owner)
}
