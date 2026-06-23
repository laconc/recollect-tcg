//! Resolved player choices: the per-ChoiceEffect handlers `apply_choice_effect` dispatches to.
//! A sibling of `clause.rs`; `use super::*` pulls shared helpers.
use super::*;

pub(crate) fn choice_damage(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    effect: ChoiceEffect,
    tile: u8,
    owner: Seat,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::Damage { amount } = effect else {
        unreachable!()
    };
    let catalog = &ctx.catalog.clone();
    push(sim, evs, Event::EffectDamaged { tile, amount });
    if sim
        .spirit_at(tile)
        .map(|sp| sp.hp <= 0 && !sp.fading)
        .unwrap_or(false)
    {
        banish_or_replace(sim, evs, catalog, tile, owner);
    }
}

pub(crate) fn choice_displace_from(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
    owner: Seat,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::DisplaceFrom { tiles } = effect else {
        unreachable!()
    };
    // Step 1 resolved: `tile` is the spirit to move. Open the destination
    // choice over empty, non-faded, terrain-free tiles within `tiles` of it (a spirit
    // may never land on a Landmark / Fabrication — invariant #1).
    let options: Vec<u8> = (0..sim.board.len() as u8)
        .filter(|&d| {
            d != tile
                && manhattan(tile, d) <= tiles as i16
                // An open landing only — no spirit, terrain (inv #1), or Stray (1b).
                && tile_open_for_arrival(sim, d)
        })
        .collect();
    if !options.is_empty() {
        push(
            sim,
            evs,
            Event::ChoiceOffered {
                choice: crate::state::PendingChoice::Target {
                    seat: owner,
                    options,
                    effect: CE::MoveTo { from: tile },
                    source: tile,
                },
            },
        );
    }
}

pub(crate) fn choice_pair_buff_step1(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
    owner: Seat,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::PairBuffStep1 {
        attack,
        defense,
        this_round,
    } = effect
    else {
        unreachable!()
    };
    // Step 1 resolved: `tile` is the first ally. Open a choice over its
    // adjacent allies (the second of the pair).
    let options: Vec<u8> = (0..sim.board.len() as u8)
        .filter(|&j| {
            j != tile
                && manhattan(tile, j) == 1
                && sim
                    .spirit_at(j)
                    .map(|s| !s.fading && s.owner == owner)
                    .unwrap_or(false)
        })
        .collect();
    if !options.is_empty() {
        push(
            sim,
            evs,
            Event::ChoiceOffered {
                choice: crate::state::PendingChoice::Target {
                    seat: owner,
                    options,
                    effect: CE::PairBuffWith {
                        from: tile,
                        attack,
                        defense,
                        this_round,
                    },
                    source: tile,
                },
            },
        );
    }
}

pub(crate) fn choice_pair_buff_with(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::PairBuffWith {
        from,
        attack,
        defense,
        this_round,
    } = effect
    else {
        unreachable!()
    };
    // Step 2 resolved: buff both allies (this round, or permanently).
    for t in [from, tile] {
        if this_round {
            push(
                sim,
                evs,
                Event::EffectTempStat {
                    tile: t,
                    attack,
                    defense,
                    until_round: sim.round,
                },
            );
        } else {
            push(
                sim,
                evs,
                Event::EffectStat {
                    tile: t,
                    attack,
                    defense,
                    form: 0,
                },
            );
        }
    }
}

pub(crate) fn choice_swap_step1(sim: &mut GameState, evs: &mut Vec<Event>, tile: u8, owner: Seat) {
    use crate::state::ChoiceEffect as CE;
    // Step 1 resolved: `tile` is one enemy. Open a choice over the enemies
    // adjacent to it (the other to swap with).
    let options: Vec<u8> = (0..sim.board.len() as u8)
        .filter(|&j| {
            j != tile
                && manhattan(tile, j) == 1
                && sim
                    .spirit_at(j)
                    .map(|s| !s.fading && s.owner != owner)
                    .unwrap_or(false)
        })
        .collect();
    if !options.is_empty() {
        push(
            sim,
            evs,
            Event::ChoiceOffered {
                choice: crate::state::PendingChoice::Target {
                    seat: owner,
                    options,
                    effect: CE::SwapWith { from: tile },
                    source: tile,
                },
            },
        );
    }
}

pub(crate) fn choice_heal_bonded_pair(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::HealBondedPair { amount } = effect else {
        unreachable!()
    };
    // `tile` is one endpoint; heal both ends of its Bond.
    if let Some(b) = sim
        .bonds
        .iter()
        .find(|b| b.tile_a == tile || b.tile_b == tile)
    {
        let (a, c) = (b.tile_a, b.tile_b);
        for t in [a, c] {
            push(sim, evs, Event::EffectRestored { tile: t, amount });
        }
    }
}

pub(crate) fn choice_reveal_fabrication_at(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    tile: u8,
) {
    let catalog = &ctx.catalog.clone();
    if sim.board[tile as usize]
        .terrain
        .as_ref()
        .map(|tr| tr.face_down)
        .unwrap_or(false)
    {
        push(sim, evs, Event::FabricationRevealed { tile });
        buff_on_fab_reveal(sim, evs, catalog);
    }
}

pub(crate) fn choice_reach_buff(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
    owner: Seat,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::ReachBuff {
        forward,
        all_directions,
    } = effect
    else {
        unreachable!()
    };
    if sim.spirit_at(tile).is_some() {
        let until = sim.round;
        push(
            sim,
            evs,
            Event::ReachBuffed {
                seat: owner,
                forward,
                all_directions,
                until_round: until,
                targeting_only: false,
                tile: Some(tile),
            },
        );
    }
}

pub(crate) fn choice_engage_from(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    effect: ChoiceEffect,
    tile: u8,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::EngageFrom { from, bonus } = effect else {
        unreachable!()
    };
    // A chosen extra engage (Again!, Reckless Charge): a single full exchange.
    if sim.spirit_at(from).map(|s| !s.fading).unwrap_or(false) && sim.spirit_at(tile).is_some() {
        full_exchange(sim, evs, ctx, from, tile, StrikeKind::Engage, bonus);
    }
}

pub(crate) fn choice_engage_step1(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    effect: ChoiceEffect,
    tile: u8,
    owner: Seat,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::EngageStep1 { bonus } = effect else {
        unreachable!()
    };
    let catalog = &ctx.catalog.clone();
    // Reckless Charge: `tile` is the chosen engager — open a target choice over
    // the enemies in its reach (→ EngageFrom).
    if let Some(sp) = sim.spirit_at(tile) {
        let reach = targeting_reach(
            sim,
            catalog,
            card(catalog, sp.card).reach,
            tile,
            owner,
            sim.board_w,
        );
        let options: Vec<u8> = reach
            .into_iter()
            .filter(|&t| {
                sim.spirit_at(t)
                    .map(|s| s.owner != owner && !s.fading)
                    .unwrap_or(false)
            })
            .collect();
        if !options.is_empty() {
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect: CE::EngageFrom { from: tile, bonus },
                        source: tile,
                    },
                },
            );
        }
    }
}

pub(crate) fn choice_retaliate_this_round(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::RetaliateThisRound { delta } = effect else {
        unreachable!()
    };
    if sim.spirit_at(tile).is_some() {
        let until = sim.round;
        push(
            sim,
            evs,
            Event::RetaliationBuffed {
                tile,
                delta,
                until_round: until,
            },
        );
    }
}

pub(crate) fn choice_pay_engage(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    effect: ChoiceEffect,
    tile: u8,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::PayEngage { from, hp, bonus } = effect else {
        unreachable!()
    };
    // Ragewoken Bison: choosing the source itself declines; otherwise pay HP
    // (self-damage) and engage the chosen target.
    if tile != from
        && sim.spirit_at(from).map(|s| !s.fading).unwrap_or(false)
        && sim.spirit_at(tile).is_some()
    {
        push(
            sim,
            evs,
            Event::EffectDamaged {
                tile: from,
                amount: hp,
            },
        );
        full_exchange(sim, evs, ctx, from, tile, StrikeKind::Engage, bonus);
    }
}

pub(crate) fn choice_peek_fabrication(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    tile: u8,
    owner: Seat,
) {
    // Curio Fox: privately record the chosen face-down Fabrication for the owner.
    if let Some(terr) = sim.board[tile as usize]
        .terrain
        .as_ref()
        .filter(|tr| tr.face_down && tr.kind == crate::state::TerrainKind::Fabrication)
    {
        let card = terr.card;
        push(
            sim,
            evs,
            Event::FabricationPeeked {
                seat: owner,
                tile,
                card,
            },
        );
    }
}

pub(crate) fn choice_re_trigger_parting(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    tile: u8,
    owner: Seat,
) {
    let catalog = &ctx.catalog.clone();
    // Second Farewell: fire the chosen ally's Parting again (it stays standing).
    if let Some(sp) = sim.spirit_at(tile).filter(|s| !s.fading) {
        let name = card(catalog, sp.card).name.clone();
        fire_doctrine(
            sim,
            evs,
            catalog,
            &name,
            crate::effects::Trigger::Parting,
            Some(tile),
            owner,
        );
    }
}

pub(crate) fn choice_stat_delta(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    effect: ChoiceEffect,
    tile: u8,
) {
    use crate::state::ChoiceEffect as CE;
    let CE::StatDelta {
        attack,
        defense,
        form,
        this_round,
    } = effect
    else {
        unreachable!()
    };
    if this_round {
        let until = sim.round;
        push(
            sim,
            evs,
            Event::EffectTempStat {
                tile,
                attack,
                defense,
                until_round: until,
            },
        );
    } else {
        push(
            sim,
            evs,
            Event::EffectStat {
                tile,
                attack,
                defense,
                form,
            },
        );
    }
}
