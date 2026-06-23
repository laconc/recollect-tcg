//! Spellbook command handlers: Ritual / Bond / Landmark / Fabrication.
//! A sibling of `decide.rs`; `use super::*` shares helpers + crate types.
use super::*;

pub(crate) fn decide_cast_ritual(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::CastRitual { hand_index } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    let p = state.player_slot(actor_slot);
    let cid = *p
        .hand
        .get(*hand_index as usize)
        .ok_or(Reject::BadHandIndex)?;
    let def = card(&ctx.catalog, cid).clone();
    if def.kind != CardKind::Ritual {
        return Err(Reject::WrongCardKind);
    }
    // A one-shot Ritual discount (Star-Strewn Otter) reduces this cast, then spends.
    let discount = state.next_ritual_discount[actor as usize] as i16;
    let eff_cost =
        (def.cost as i16 + cost_aura(state, actor, &ctx.catalog) - discount).max(0) as u8;
    if p.anima < eff_cost {
        return Err(Reject::NotEnoughAnima);
    }
    if discount > 0 {
        push(
            &mut sim,
            &mut evs,
            Event::RitualDiscountConsumed { seat: actor },
        );
    }
    push(
        &mut sim,
        &mut evs,
        Event::AnimaSpent {
            seat: actor,
            amount: eff_cost,
        },
    );
    spend_card_tax(&mut sim, &mut evs, actor);
    push(
        &mut sim,
        &mut evs,
        Event::RitualCast {
            seat: actor,
            card: cid,
        },
    );
    // The Ritual's clauses fire through the same executor as OnPlay.
    fire_effects(
        &mut sim,
        &mut evs,
        ctx,
        crate::effects::Trigger::OnPlay,
        &def.name,
        None,
        actor,
    );
    // Otterling Magus: if this ritual opened a target choice and the caster controls
    // Otterling, arm one extra target — applied when the choice resolves.
    if matches!(
        sim.pending_choice,
        Some(crate::state::PendingChoice::Target { .. })
    ) && exception_active(
        &sim,
        &ctx.catalog,
        actor,
        crate::effects::RuleException::RitualsExtraTarget,
    ) {
        push(
            &mut sim,
            &mut evs,
            Event::RitualExtraTargetsArmed { count: 1 },
        );
    }
    // Patience: schedule any "1 Anima at your next Flow" the card promises.
    schedule_flow_anima(&mut sim, &mut evs, &ctx.catalog, &def.name, actor);
    Ok(evs)
}

/// The Solace plays an Unwriting EVENT from hand — a one-shot whose clauses fire through
/// the same OnPlay executor as a Ritual, then it's discarded. (Mirrors `decide_cast_ritual`.)
pub(crate) fn decide_tell_unwriting(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::TellUnwriting { hand_index } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    let p = state.player_slot(actor_slot);
    let cid = *p
        .hand
        .get(*hand_index as usize)
        .ok_or(Reject::BadHandIndex)?;
    let def = card(&ctx.catalog, cid).clone();
    if def.kind != CardKind::Unwriting {
        return Err(Reject::WrongCardKind);
    }
    let eff_cost = (def.cost as i16 + cost_aura(state, actor, &ctx.catalog)).max(0) as u8;
    if p.anima < eff_cost {
        return Err(Reject::NotEnoughAnima);
    }
    push(
        &mut sim,
        &mut evs,
        Event::AnimaSpent {
            seat: actor,
            amount: eff_cost,
        },
    );
    spend_card_tax(&mut sim, &mut evs, actor);
    push(
        &mut sim,
        &mut evs,
        Event::UnwritingTold {
            seat: actor,
            card: cid,
        },
    );
    fire_effects(
        &mut sim,
        &mut evs,
        ctx,
        crate::effects::Trigger::OnPlay,
        &def.name,
        None,
        actor,
    );
    Ok(evs)
}

pub(crate) fn decide_attach_bond(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::AttachBond {
        hand_index,
        tile_a,
        tile_b,
    } = cmd
    else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    let p = state.player_slot(actor_slot);
    let cid = *p
        .hand
        .get(*hand_index as usize)
        .ok_or(Reject::BadHandIndex)?;
    let def = card(&ctx.catalog, cid).clone();
    if def.kind != CardKind::Bond {
        return Err(Reject::WrongCardKind);
    }
    // Both must be your standing spirits, adjacent to each other.
    let ok = |t: u8| matches!(state.spirit_at(t), Some(sp) if sp.owner == actor && !sp.fading);
    if !ok(*tile_a) || !ok(*tile_b) || tile_a == tile_b {
        return Err(Reject::BadTile);
    }
    if manhattan(*tile_a, *tile_b) != 1 {
        return Err(Reject::TargetNotInReach);
    }
    // Rondel's "Your Bonds cost 0": a BondsCostZero carrier zeroes the
    // anima cost of the owner's Bonds (consulted at the bond chokepoint).
    // Common Ground: free instead if EITHER endpoint stands on the owner's
    // Common Ground (a positional BondsFreeOnLandmark exception).
    let free = exception_active(
        state,
        &ctx.catalog,
        actor,
        crate::effects::RuleException::BondsCostZero,
    ) || tile_terrain_exception(
        state,
        &ctx.catalog,
        *tile_a,
        actor,
        crate::effects::RuleException::BondsFreeOnLandmark,
    ) || tile_terrain_exception(
        state,
        &ctx.catalog,
        *tile_b,
        actor,
        crate::effects::RuleException::BondsFreeOnLandmark,
    );
    let eff_cost = if free {
        0
    } else {
        (def.cost as i16 + cost_aura(state, actor, &ctx.catalog)).max(0) as u8
    };
    if p.anima < eff_cost {
        return Err(Reject::NotEnoughAnima);
    }
    if eff_cost > 0 {
        push(
            &mut sim,
            &mut evs,
            Event::AnimaSpent {
                seat: actor,
                amount: eff_cost,
            },
        );
        spend_card_tax(&mut sim, &mut evs, actor);
    }
    push(
        &mut sim,
        &mut evs,
        Event::BondAttached {
            seat: actor,
            card: cid,
            tile_a: *tile_a,
            tile_b: *tile_b,
        },
    );
    // OnYouPlayBond: the active seat's spirits that react to their teller
    // playing a Bond (Linnet of the Lea draws). Names gathered before firing.
    let watchers: Vec<(u8, String)> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit
                .as_ref()
                .filter(|sp| !sp.fading && sp.owner == actor)
                .map(|sp| (i as u8, card(&ctx.catalog, sp.card).name.clone()))
        })
        .collect();
    for (tile, name) in watchers {
        fire_effects(
            &mut sim,
            &mut evs,
            ctx,
            crate::effects::Trigger::OnYouPlayBond,
            &name,
            Some(tile),
            actor,
        );
    }
    Ok(evs)
}

pub(crate) fn decide_place_landmark(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::PlaceLandmark { hand_index, tile } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    let p = state.player_slot(actor_slot);
    let cid = *p
        .hand
        .get(*hand_index as usize)
        .ok_or(Reject::BadHandIndex)?;
    let def = card(&ctx.catalog, cid).clone();
    if def.kind != CardKind::Landmark {
        return Err(Reject::WrongCardKind);
    }
    let t = &state.board[*tile as usize];
    // A Stray occupies its tile (§6 — it lives in `state.stray`, not `board.spirit`), so it
    // is "held" for placement just like a spirit/terrain: terrain may not be set onto it
    // (else a later Overwrite-onto-Stray lands a spirit atop the terrain — the illegal
    // spirit+terrain coexistence). The sibling of the spirit-onto-Stray guard in
    // `decide_play_spirit`; mirrors `tile_open_for_arrival`'s Stray clause.
    let stray_here = state
        .stray
        .as_ref()
        .map(|s| s.tile == *tile)
        .unwrap_or(false);
    if t.faded || t.spirit.is_some() || t.terrain.is_some() || stray_here {
        return Err(Reject::TileHeld);
    }
    // Landmarks are placed under spirit legality: projected or margin.
    let proj = projection(state, actor, &ctx.catalog);
    let margin = !any_projected_placement(state, actor, &ctx.catalog);
    if !(proj[*tile as usize] || margin) {
        return Err(Reject::OutsideProjection);
    }
    let eff_cost = (def.cost as i16 + cost_aura(state, actor, &ctx.catalog)).max(0) as u8;
    if p.anima < eff_cost {
        return Err(Reject::NotEnoughAnima);
    }
    push(
        &mut sim,
        &mut evs,
        Event::AnimaSpent {
            seat: actor,
            amount: eff_cost,
        },
    );
    spend_card_tax(&mut sim, &mut evs, actor);
    push(
        &mut sim,
        &mut evs,
        Event::LandmarkPlaced {
            seat: actor,
            card: cid,
            tile: *tile,
        },
    );
    Ok(evs)
}

pub(crate) fn decide_set_fabrication(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::SetFabrication { hand_index, tile } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    let p = state.player_slot(actor_slot);
    let cid = *p
        .hand
        .get(*hand_index as usize)
        .ok_or(Reject::BadHandIndex)?;
    let def = card(&ctx.catalog, cid).clone();
    if def.kind != CardKind::Fabrication {
        return Err(Reject::WrongCardKind);
    }
    let t = &state.board[*tile as usize];
    // A Stray holds its tile — a Fabrication may not be set onto it (same coexistence guard
    // as the Landmark path above; the wild lives in `state.stray`, off-board of `board`).
    let stray_here = state
        .stray
        .as_ref()
        .map(|s| s.tile == *tile)
        .unwrap_or(false);
    if t.faded || t.spirit.is_some() || t.terrain.is_some() || stray_here {
        return Err(Reject::TileHeld);
    }
    let proj = projection(state, actor, &ctx.catalog);
    let margin = !any_projected_placement(state, actor, &ctx.catalog);
    if !(proj[*tile as usize] || margin) {
        return Err(Reject::OutsideProjection);
    }
    // The Long Shadow: a Fabrication placed adjacent to it costs less.
    let eff_cost = (def.cost as i16
        + cost_aura(state, actor, &ctx.catalog)
        + adjacent_landmark_fab_delta(state, &ctx.catalog, actor, *tile))
    .max(0) as u8;
    if p.anima < eff_cost {
        return Err(Reject::NotEnoughAnima);
    }
    push(
        &mut sim,
        &mut evs,
        Event::AnimaSpent {
            seat: actor,
            amount: eff_cost,
        },
    );
    spend_card_tax(&mut sim, &mut evs, actor);
    push(
        &mut sim,
        &mut evs,
        Event::FabricationSet {
            seat: actor,
            card: cid,
            tile: *tile,
        },
    );
    Ok(evs)
}
