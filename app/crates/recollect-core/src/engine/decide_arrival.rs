//! Arrival + movement command handlers: PlaySpirit / Overwrite / StrikeFabrication / MoveSpirit.
//! A sibling of `decide.rs`; `use super::*` shares helpers + crate types.
use super::*;

pub(crate) fn decide_play_spirit(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::PlaySpirit {
        hand_index,
        tile,
        engage,
        chain_prefs,
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
    let idx = *hand_index as usize;
    if idx >= p.hand.len() {
        return Err(Reject::BadHandIndex);
    }
    // Borrow the def (no clone) and clone only its name for the post-engage OnPlay below;
    // the catalog is immutable behind Arc, and every scalar read lands before the &mut ctx
    // engage path that follows.
    let def = card(&ctx.catalog, p.hand[idx]);
    let def_name = def.name.clone();
    // Affordability is gated below against the cost-AURA-adjusted cost
    // (a cost-reducing aura can make a raw-too-expensive card playable),
    // so we must not reject on raw def.cost here.
    if (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    let t = &state.board[*tile as usize];
    if t.faded {
        return Err(Reject::TileFaded);
    }
    // The Quiet Spreads (Unwriting): a tile gone calm cannot be Played onto (from the
    // scheduled round onward) — it rejects placement like a faded tile.
    if state
        .calm_tiles
        .iter()
        .any(|&(ct, r)| ct == *tile && state.round >= r)
    {
        return Err(Reject::TileFaded);
    }
    if t.spirit.is_some() {
        return Err(Reject::TileOccupied);
    }
    // §4: a spirit is placed on an EMPTY tile — terrain (a Landmark or revealed
    // Fabrication) is not empty, so a spirit may not land on it (the symmetric rule
    // to `decide_place_landmark`/`decide_set_fabrication`, which both reject a
    // terrain-occupied tile). Without this a tile adjacent to your own Landmark —
    // in your projection — admitted a spirit onto a second terrain tile, creating
    // the illegal `spirit AND terrain coexist` state. (Stepping a Mobile spirit onto
    // an enemy face-down Fabrication springs it from `from` and never arrives, so the
    // Move path is unaffected; a Play never targets a face-down lie.)
    if t.terrain.is_some() {
        return Err(Reject::TileOccupied);
    }
    // §2/§6: a Stray stands on its tile (it lives in `state.stray`, not `board.spirit`),
    // so that tile is NOT empty — a spirit may not be *placed* onto it. To contest a
    // Stray's tile you Overwrite (which reaches the wild), never Play. Without this a Play
    // would drop a spirit on top of the Stray — the illegal spirit-AND-stray coexistence.
    if state
        .stray
        .as_ref()
        .map(|s| s.tile == *tile)
        .unwrap_or(false)
    {
        return Err(Reject::TileOccupied);
    }
    // Player one's very first placement: the home two rows — while
    // they exist. Once the Memory contracts, the Margin Rule governs.
    if actor == Seat::A && !state.player_a.first_placement_done {
        let (_, y) = tile_xy_w(*tile, state.board_w);
        if !state.contracted && !Seat::A.home_rows_w(state.board_w).contains(&y) {
            return Err(Reject::FirstPlacementHomeRows);
        }
    }
    // Rooted Telling + the Margin Rule.
    let proj = projection(state, actor, &ctx.catalog);
    if !proj[*tile as usize] && any_projected_placement(state, actor, &ctx.catalog) {
        return Err(Reject::OutsideProjection);
    }
    if let Some(target) = engage {
        let reach = targeting_reach(state, &ctx.catalog, def.reach, *tile, actor, state.board_w);
        if !reach.contains(target) {
            return Err(Reject::TargetNotInReach);
        }
        match state.spirit_at(*target) {
            Some(sp) if sp.owner != actor && !sp.fading => {}
            Some(_) => return Err(Reject::TargetNotEnemy),
            None => return Err(Reject::TileEmpty),
        }
    }
    if def.lurk && engage.is_some() {
        return Err(Reject::TargetNotInReach); // the unspoken cannot strike
    }
    let eff_cost = (def.cost as i16 + cost_aura(state, actor, &ctx.catalog)).max(0) as u8;
    if state.player_slot(actor_slot).anima < eff_cost {
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
        Event::SpiritPlayed {
            seat: actor,
            card: def.id,
            tile: *tile,
            attack: def.attack,
            defense: def.defense,
            hp: def.hp,
            face_down: def.lurk,
        },
    );
    // Kindle: the queued next-arrival buff lands before this spirit strikes.
    apply_next_arrival(&mut sim, &mut evs, actor, *tile);
    let mut banished = false;
    if let Some(target) = engage {
        banished = full_exchange(
            &mut sim,
            &mut evs,
            ctx,
            *tile,
            *target,
            StrikeKind::Engage,
            0,
        );
    }
    // The Arrival Law's ordering: engage → INTERCEPTION → chain decision.
    interception(&mut sim, &mut evs, ctx, *tile, actor);
    if banished {
        // the arrival's chain preference list steers each link.
        momentum_prefs(&mut sim, &mut evs, ctx, *tile, actor, chain_prefs);
    }
    // Again!: a queued second engage offers the arrival another target.
    offer_second_engage(&mut sim, &mut evs, &ctx.catalog, actor, *tile, *engage);
    check_throughline(&mut sim, &mut evs, &ctx.catalog, *tile, actor);
    // OnPlay effects resolve AFTER the full arrival sequence, if the
    // arriver still stands.
    if sim.spirit_at(*tile).map(|sp| !sp.fading).unwrap_or(false) {
        fire_effects(
            &mut sim,
            &mut evs,
            ctx,
            crate::effects::Trigger::OnPlay,
            &def_name,
            Some(*tile),
            actor,
        );
    }
    Ok(evs)
}

pub(crate) fn decide_overwrite(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Overwrite { hand_index, tile } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    let p = state.player_slot(actor_slot);
    let idx = *hand_index as usize;
    if idx >= p.hand.len() {
        return Err(Reject::BadHandIndex);
    }
    // Borrow the def (no clone): every read is a scalar landing before the &mut ctx
    // engage path, and the catalog is immutable behind Arc.
    let def = card(&ctx.catalog, p.hand[idx]);
    // Affordability is gated below against the cost-AURA-adjusted cost
    // (a cost-reducing aura can make a raw-too-expensive card playable),
    // so we must not reject on raw def.cost here.
    if (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    let occupant = match state.spirit_at(*tile) {
        Some(sp) if sp.owner != actor && !sp.fading => sp.clone(),
        Some(_) => return Err(Reject::TargetNotEnemy),
        // §2: a Stray stands on its tile (it lives in `state.stray`, not `board.spirit`),
        // so an Overwrite reaches it — a revealed Stray is fought, a hidden one is denied
        // entry. The spirit slot is empty here; route to the Stray handler if one stands
        // on this tile, else it really is an empty tile.
        None => {
            if state
                .stray
                .as_ref()
                .map(|s| s.tile == *tile)
                .unwrap_or(false)
            {
                return decide_overwrite_stray(state, *hand_index, *tile, ctx);
            }
            return Err(Reject::TileEmpty);
        }
    };
    // Overwrite reads the ACTING SLOT's own projection (2v2:
    // your spirits, not your partner's). Equals the team projection in 1v1.
    let proj = projection_slot(state, actor_slot, &ctx.catalog);
    if !proj[*tile as usize] {
        return Err(Reject::OutsideProjection);
    }
    if state.contracted && is_rim_w(*tile, state.board_w) {
        return Err(Reject::TileHeld); // ground too thin to write on
    }
    let occ_def = card(&ctx.catalog, occupant.card);
    // Simultaneous exchange; the overwriter arrives at full HP — only
    // the defender can Echo. Variance flows to the besieged.
    let d_echo = occupant.echo_eligible() && ctx.entropy.draw_below(ECHO_NUM, ECHO_DEN);
    let a_edge = if def.resonance.edge_over(occ_def.resonance) {
        EDGE
    } else {
        0
    };
    let d_edge = if occ_def.resonance.edge_over(def.resonance) {
        EDGE
    } else {
        0
    };
    let dmg_def = (def.attack + a_edge
        - eff_defense(
            occupant.defense,
            def.arcane,
            eff_warded(state, &ctx.catalog, *tile),
        ))
    .max(0);
    let dmg_att = (occupant.attack + d_edge + if d_echo { ECHO_BONUS } else { 0 }
        - eff_defense(def.defense, occ_def.arcane, def.warded))
    .max(0);
    let success = occupant.hp - dmg_def <= 0;
    push(
        &mut sim,
        &mut evs,
        Event::AnimaSpent {
            seat: actor,
            amount: def.cost,
        },
    );
    push(
        &mut sim,
        &mut evs,
        Event::Overwrote {
            seat: actor,
            card: def.id,
            tile: *tile,
            success,
            damage_to_defender: dmg_def,
            defender_echo: d_echo,
            attack: def.attack,
            defense: def.defense,
            attacker_hp_left: def.hp - dmg_att,
            attacker_hp_max: def.hp,
        },
    );
    if success {
        // The overwriter took the tile from the banished occupant. Any Bond that occupant
        // was part of is now stale — its endpoint holds an ENEMY (the overwriter), not the
        // bond owner's spirit — so break it BEFORE Momentum chains. Otherwise the dead
        // occupant's Promise could redirect a chain blow onto the overwriter that replaced
        // it (the `damage_redirect_to_partner` reaching across the stolen tile).
        prune_broken_bonds(&mut sim, &mut evs, &ctx.catalog);
    }
    if success && def.hp - dmg_att > 0 {
        interception(&mut sim, &mut evs, ctx, *tile, actor);
        momentum(&mut sim, &mut evs, ctx, *tile, actor);
    }
    Ok(evs)
}

/// §2 — Overwrite onto a **Stray** (it stands on its tile; the unclaimed Stray lives in
/// `state.stray`, not `board.spirit`). A **revealed** Stray is fought in one simultaneous
/// exchange against its stats (a Stray never engages and never Echoes, §1, so the exchange
/// is deterministic — no defender Echo roll, no entropy drawn). A **hidden** Stray (a
/// veiled Wary, or any Stray not yet surfaced face-up) is **denied entry**: it leaves with
/// no impression and no reveal, then the overwriter takes the cleared tile uncontested. The
/// projection/rim gates mirror the spirit path; the cost-spend mirrors it too (raw
/// `def.cost`, exactly like a spirit Overwrite). Redaction: the hidden-deny path emits only
/// `StrayDenied { tile }` (no `CardId`), so a veiled Stray's identity never leaks.
fn decide_overwrite_stray(
    state: &GameState,
    hand_index: u8,
    tile: u8,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    // Borrow the def (no clone): every read is a scalar landing before the &mut ctx
    // engage path, and the catalog is immutable behind Arc.
    let def = card(
        &ctx.catalog,
        state.player_slot(actor_slot).hand[hand_index as usize],
    );
    let Some(stray) = state.stray.clone().filter(|s| s.tile == tile) else {
        return Err(Reject::TileEmpty);
    };
    // Overwrite reads the acting slot's own projection (team in 1v1). Strays surface on
    // inner tiles only, so the rim/contracted check never bites — but keep it for symmetry.
    let proj = projection_slot(state, actor_slot, &ctx.catalog);
    if !proj[tile as usize] {
        return Err(Reject::OutsideProjection);
    }
    if state.contracted && is_rim_w(tile, state.board_w) {
        return Err(Reject::TileHeld);
    }
    // Pay full cost (raw `def.cost`, as the spirit Overwrite does).
    push(
        &mut sim,
        &mut evs,
        Event::AnimaSpent {
            seat: actor,
            amount: def.cost,
        },
    );
    if stray.veiled {
        // A hidden thing is denied entry — it leaves (no impression, no reveal, nothing),
        // and the overwriter takes the cleared tile as an uncontested arrival. The denial
        // MUST NOT name the Stray (redaction): `StrayDenied` carries only the tile.
        push(&mut sim, &mut evs, Event::StrayDenied { tile });
        push(
            &mut sim,
            &mut evs,
            Event::OverwroteStray {
                seat: actor,
                card: def.id,
                tile,
                success: true, // it lands on the cleared tile, unwounded
                damage_to_stray: 0,
                attack: def.attack,
                defense: def.defense,
                attacker_hp_left: def.hp,
                attacker_hp_max: def.hp,
            },
        );
        // An Overwrite is an arrival: other interceptors may strike the newcomer (§2). But
        // Momentum does NOT fire — it is granted by a *defeat*, and a denied-entry hidden
        // thing was never fought (it left; there was no banish). So only the interception
        // step runs here, not the chain.
        interception(&mut sim, &mut evs, ctx, tile, actor);
        return Ok(evs);
    }
    // A revealed Stray is fought. One simultaneous exchange; the overwriter arrives at full
    // HP and a Stray never Echoes — so no defender variance, no entropy is consulted.
    let sc = card(&ctx.catalog, stray.card);
    // Strays are Resonance-less (Neutral), so neither side takes a wheel edge against one.
    // A Stray fights at its printed stats (no stat-mod state — same as the Feral
    // interception path); its living HP is tracked in `stray.hp`.
    let dmg_stray = (def.attack - eff_defense(sc.defense, def.arcane, sc.warded)).max(0);
    let dmg_att = (sc.attack - eff_defense(def.defense, sc.arcane, def.warded)).max(0);
    let success = stray.hp - dmg_stray <= 0;
    push(
        &mut sim,
        &mut evs,
        Event::OverwroteStray {
            seat: actor,
            card: def.id,
            tile,
            success,
            damage_to_stray: dmg_stray,
            attack: def.attack,
            defense: def.defense,
            attacker_hp_left: def.hp - dmg_att,
            attacker_hp_max: def.hp,
        },
    );
    // If the overwriter banished the wild AND still stands, it is an arrival like any
    // other: interceptors may strike it, and Momentum may chain.
    if success && def.hp - dmg_att > 0 {
        interception(&mut sim, &mut evs, ctx, tile, actor);
        momentum(&mut sim, &mut evs, ctx, tile, actor);
    }
    Ok(evs)
}

pub(crate) fn decide_strike_fabrication(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::StrikeFabrication { from, tile } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    if (*from as usize) >= state.board.len() || (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    let sp = match state.spirit_at(*from) {
        Some(sp) if sp.owner == actor && !sp.fading && !sp.face_down => sp.clone(),
        Some(_) => return Err(Reject::NotYourSpirit),
        None => return Err(Reject::TileEmpty),
    };
    // The lie must be an ENEMY face-down Fabrication in this spirit's reach. A borrow
    // suffices (only the reach scalar is read, before the &mut ctx spring path).
    let def = card(&ctx.catalog, sp.card);
    if !targeting_reach(state, &ctx.catalog, def.reach, *from, actor, state.board_w).contains(tile)
    {
        return Err(Reject::TargetNotInReach);
    }
    match &state.board[*tile as usize].terrain {
        Some(terr)
            if terr.kind == crate::state::TerrainKind::Fabrication
                && terr.face_down
                && terr.owner != actor => {}
        _ => return Err(Reject::BadTile),
    }
    // Spring it from range — the striker is the engager, and stays put.
    spring_fabrication(&mut sim, &mut evs, ctx, *tile, Some(*from));
    Ok(evs)
}

pub(crate) fn decide_move_spirit(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::MoveSpirit { from, to, engage } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    if (*from as usize) >= state.board.len() || (*to as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    let sp = match state.spirit_at(*from) {
        Some(sp) => sp.clone(),
        None => return Err(Reject::TileEmpty),
    };
    if sp.owner != actor {
        return Err(Reject::NotYourSpirit);
    }
    if sp.fading {
        return Err(Reject::SpiritFading);
    }
    // A borrow suffices (only the reach scalar is read, before the &mut ctx engage path);
    // the catalog is immutable behind Arc.
    let def = card(&ctx.catalog, sp.card);
    if !keyword_active(state, &ctx.catalog, *from, crate::effects::Keyword::Mobile) {
        return Err(Reject::NotMobile);
    }
    if restricted(state, actor, crate::effects::Restriction::Move) {
        return Err(Reject::MovementRestricted);
    }
    // A Mobile spirit moves at most once per turn, and never the turn it arrived. Both are
    // recorded by its current tile sitting in `moved_this_turn` (a move/arrival adds the destination).
    if state.moved_this_turn.contains(from) {
        return Err(Reject::AlreadyMoved);
    }
    if !adjacent4_w(*from, state.board_w).any(|t| t == *to) {
        return Err(Reject::NotAdjacent);
    }
    let t = &state.board[*to as usize];
    if t.faded {
        return Err(Reject::TileFaded);
    }
    if t.spirit.is_some() {
        return Err(Reject::TileOccupied);
    }
    // §2/§6: a Stray stands on its tile, so a Mobile step may not land there either — the
    // destination is occupied by the wild. (To contest a Stray you Overwrite from hand, not
    // a Move.) Without this a step would put a spirit on top of the Stray — the illegal
    // spirit-AND-stray coexistence (caught by invariant 1b in the full-catalog playthrough).
    if state.stray.as_ref().map(|s| s.tile == *to).unwrap_or(false) {
        return Err(Reject::TileOccupied);
    }
    // Follow-up: stepping into a lie. If the destination holds an
    // ENEMY face-down Fabrication, the mover springs it — the spirit
    // does NOT take the tile; the trap fires on it where it stands.
    if let Some(terr) = &t.terrain {
        let enemy_trap = terr.kind == crate::state::TerrainKind::Fabrication
            && terr.face_down
            && terr.owner != actor;
        if enemy_trap {
            if engage.is_some() {
                return Err(Reject::TargetNotInReach);
            }
            // The engager is the mover, still at `from` (it never arrives).
            spring_fabrication(&mut sim, &mut evs, ctx, *to, Some(*from));
            return Ok(evs);
        }
        // Your own terrain (or a revealed Landmark) simply blocks.
        return Err(Reject::TileOccupied);
    }
    if let Some(target) = engage {
        // Don't Look: the mover may relocate, but can't strike out this round.
        if sp.no_engage_until >= state.round {
            return Err(Reject::EngageRestricted);
        }
        let reach = targeting_reach(state, &ctx.catalog, def.reach, *to, actor, state.board_w);
        if !reach.contains(target) {
            return Err(Reject::TargetNotInReach);
        }
        match state.spirit_at(*target) {
            Some(e) if e.owner != actor && !e.fading => {}
            Some(_) => return Err(Reject::TargetNotEnemy),
            None => return Err(Reject::TileEmpty),
        }
    }
    push(
        &mut sim,
        &mut evs,
        Event::SpiritMoved {
            from: *from,
            to: *to,
        },
    );
    fade_if_held(&mut sim, &mut evs, *from);
    let mut banished = false;
    if let Some(target) = engage {
        banished = full_exchange(&mut sim, &mut evs, ctx, *to, *target, StrikeKind::Engage, 0);
    }
    interception(&mut sim, &mut evs, ctx, *to, actor);
    if banished {
        momentum(&mut sim, &mut evs, ctx, *to, actor);
    }
    // a move may complete a Throughline through the new tile.
    check_throughline(&mut sim, &mut evs, &ctx.catalog, *to, actor);
    Ok(evs)
}
