//! The command -> events decision: legality checks + simulated resolution.
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

/// The body of [`GameState::decide`]: a big match over `Command`, one arm per
/// move. Each arm validates (ownership, reach, projection, affordability,
/// phase) and `push`es the resulting events onto a working clone so later
/// steps observe earlier ones. Returns `Reject` on any illegal move. This is
/// where ~all rules live; a new command gets a new arm here.
pub(crate) fn decide_impl(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    if matches!(state.phase, Phase::Finished { .. }) {
        return Err(Reject::MatchOver);
    }
    // System forfeit: resolvable on EITHER seat's turn, so it precedes
    // the turn-ownership check below. We record the standing tally for the ledger; the
    // win itself is by forfeit (the present player), set in `evolve`.
    if let Command::MatchAbandoned { seat } = cmd {
        let mut sim = state.clone();
        let mut evs = Vec::new();
        let (mut score_a, mut score_b) = (0u8, 0u8);
        for t in &sim.board {
            if let Some(sp) = &t.spirit
                && !sp.fading
            {
                match sp.owner {
                    Seat::A => score_a += 1,
                    Seat::B => score_b += 1,
                }
            }
        }
        push(
            &mut sim,
            &mut evs,
            Event::MatchAbandoned {
                seat: *seat,
                score_a,
                score_b,
            },
        );
        return Ok(evs);
    }
    if ctx.actor != state.active {
        return Err(Reject::NotYourTurn);
    }
    // The acting player is the active SLOT (2v2 A1≠A2); hand/anima reads go
    // through `player_slot(actor_slot)`, never the bare team player.
    let actor_slot = state.active_slot;
    // The hand-cap window admits exactly one command.
    if let Phase::PendingRelease { seat, .. } = state.phase {
        match cmd {
            Command::Release { hand_index } => {
                if seat != ctx.actor {
                    return Err(Reject::NotYourTurn);
                }
                if (*hand_index as usize) >= state.player_slot(actor_slot).hand.len() {
                    return Err(Reject::BadHandIndex);
                }
                let mut sim = state.clone();
                let mut evs = Vec::new();
                push(
                    &mut sim,
                    &mut evs,
                    Event::CardReleased {
                        seat,
                        hand_index: *hand_index,
                    },
                );
                return Ok(evs);
            }
            _ => return Err(Reject::PendingReleaseFirst),
        }
    }
    if matches!(cmd, Command::Release { .. }) {
        return Err(Reject::NotPendingRelease);
    }

    let actor = ctx.actor;
    let mut sim = state.clone();
    let mut evs = Vec::new();

    match cmd {
        Command::Glimpse => {
            evs = decide_glimpse(state, ctx)?;
        }
        Command::CastRitual { .. } => {
            evs = decide_cast_ritual(state, cmd, ctx)?;
        }
        Command::TellUnwriting { .. } => {
            evs = decide_tell_unwriting(state, cmd, ctx)?;
        }
        Command::AttachBond { .. } => {
            evs = decide_attach_bond(state, cmd, ctx)?;
        }
        Command::PlaceLandmark { .. } => {
            evs = decide_place_landmark(state, cmd, ctx)?;
        }
        Command::SetFabrication { .. } => {
            evs = decide_set_fabrication(state, cmd, ctx)?;
        }
        Command::BanishStray => {
            evs = decide_banish_stray(state, ctx)?;
        }
        Command::Mulligan { .. } => {
            evs = decide_mulligan(state, cmd, ctx)?;
        }
        Command::Reclaim { .. } => {
            evs = decide_reclaim(state, cmd, ctx)?;
        }
        Command::Evolve { .. } => {
            evs = decide_evolve(state, cmd, ctx)?;
        }
        Command::Devolve { .. } => {
            evs = decide_devolve(state, cmd, ctx)?;
        }
        Command::Reveal { .. } => {
            evs = decide_reveal(state, cmd, ctx)?;
        }
        Command::SetOrders { .. } => {
            evs = decide_set_orders(state, cmd, ctx)?;
        }
        Command::Choose { .. } => {
            evs = decide_choose(state, cmd, ctx)?;
        }
        Command::EndTurn if state.pending_choice.is_some() => {
            return Err(Reject::ChoicePending);
        }
        Command::EndTurn => {
            end_turn(&mut sim, &mut evs, actor, ctx);
        }
        Command::PlaySpirit { .. } => {
            evs = decide_play_spirit(state, cmd, ctx)?;
        }
        Command::Overwrite { .. } => {
            evs = decide_overwrite(state, cmd, ctx)?;
        }
        Command::StrikeFabrication { .. } => {
            evs = decide_strike_fabrication(state, cmd, ctx)?;
        }
        Command::MoveSpirit { .. } => {
            evs = decide_move_spirit(state, cmd, ctx)?;
        }
        Command::Release { .. } => unreachable!("handled above"),
        Command::MatchAbandoned { .. } => {
            unreachable!("system forfeit handled before the turn-ownership check")
        }
    }
    Ok(evs)
}

/// Glimpse (§5) — the burn-then-peek-then-spend opening of the turn's one Glimpse.
/// It is no longer free: to activate it you BURN a card of your choice from your
/// hand (the activation cost — the card leaves play entirely, thinning the deck),
/// THEN peek the top card and KEEP it (stays on top, no Anima) or BOTTOM it for
/// +1 Anima. Two choices, resolved through the shared `PendingChoice`/`Choose`
/// flow: step 1 is `GlimpseBurn` (which hand card to spend); step 2 is the
/// keep-or-bottom `Glimpse`. Net: keep = −1 card for foresight; bottom = −2 cards
/// for +1 Anima.
///
/// This marks the once-per-turn flag and offers the BURN choice. The
/// keep-or-bottom `Glimpse` opens only once the burn resolves (`decide_choose`),
/// so the top is peeked then — the burn doesn't touch the deck, so the top is the
/// same card either way. Gated to a non-empty hand (nothing to burn) AND a
/// non-empty page (nothing to peek); `legal_commands` enforces both, the rejects
/// here guard the direct-call path.
fn decide_glimpse(state: &GameState, ctx: &mut TurnCtx) -> Result<Vec<Event>, Reject> {
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    if state.player_slot(actor_slot).glimpsed_this_turn {
        return Err(Reject::AlreadyGlimpsedThisTurn);
    }
    // An empty page glimpses nothing; an empty hand has nothing to burn. Both gate
    // the action out of `legal_commands` — these guard a direct (non-menu) call.
    let Some(peeked) = state.player_slot(actor_slot).deck.first().copied() else {
        return Err(Reject::NothingToPeek);
    };
    let burnable = state.player_slot(actor_slot).hand.clone();
    if burnable.is_empty() {
        return Err(Reject::NothingToBurn);
    }
    push(
        &mut sim,
        &mut evs,
        Event::Glimpsed {
            seat: actor,
            peeked: Some(peeked),
        },
    );
    // Step 1: the BURN cost — offer the choice of WHICH hand card to spend. The
    // keep-or-bottom `Glimpse` opens behind it when this resolves (`decide_choose`).
    push(
        &mut sim,
        &mut evs,
        Event::ChoiceOffered {
            choice: crate::state::PendingChoice::GlimpseBurn {
                seat: actor,
                burnable,
            },
        },
    );
    Ok(evs)
}

/// Mulligan (§5 — London-lite): the opening reshuffle. Legal ONLY in the opening
/// window — round 1, the active seat's own turn, before that seat has acted
/// (placed, Glimpsed, or spent any anima), and at most once per seat. The window
/// is 1v1-only: a 2v2 seat is two physical slots sharing one `mulliganed[seat]`
/// flag, which would let one slot's mulligan bar the other's, so it is not offered
/// there (a clean, correct extension point rather than a subtle per-team bug).
///
/// The mechanic: take the seat's hand of `H`, shuffle it back into the deck,
/// reshuffle, draw `H` fresh cards (the "full hand"), then bottom ONE — chosen
/// deterministically by a seeded roll, no player pick (the bottomed card is the
/// cost; the hand ends at `H − 1`). Every entropy draw here advances the
/// counter-mode stream, so it is journaled and identical on replay. The resulting
/// hand + deck ride the `Mulliganed` event; `evolve` applies them verbatim,
/// touching no entropy of its own (the dumb-evolve law).
fn decide_mulligan(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Mulligan { seat } = cmd else {
        unreachable!("decide_mulligan only handles Command::Mulligan")
    };
    let actor = ctx.actor;
    // The command's `seat` must be the actor's own — you mulligan your OWN hand,
    // and the actor is already verified to be the active seat (turn check above).
    if *seat != actor {
        return Err(Reject::MulliganUnavailable);
    }
    if !mulligan_window(state, actor) {
        return Err(Reject::MulliganUnavailable);
    }
    let p = state.player(actor);
    let h = p.hand.len();
    // Shuffle the hand back into the deck, then reshuffle the whole page.
    let mut pool: Vec<crate::types::CardId> = p.deck.clone();
    pool.extend(p.hand.iter().copied());
    ctx.entropy.shuffle(&mut pool);
    // Draw a fresh full hand of the same size, then bottom one (the cost).
    let draw = h.min(pool.len());
    let mut new_hand: Vec<crate::types::CardId> = pool.drain(..draw).collect();
    let mut new_deck = pool; // the rest stays on the page, in shuffled order
    if !new_hand.is_empty() {
        // The bottomed card is seed-chosen, not player-picked — a real cost the
        // teller cannot steer (keeps Mulligan a single atomic command).
        let idx = ctx.entropy.below(new_hand.len() as u64) as usize;
        let bottomed = new_hand.remove(idx);
        new_deck.push(bottomed); // to the bottom of the page
    }
    let mut sim = state.clone();
    let mut evs = Vec::new();
    push(
        &mut sim,
        &mut evs,
        Event::Mulliganed {
            seat: actor,
            hand: new_hand,
            deck: new_deck,
        },
    );
    Ok(evs)
}

/// The opening mulligan window for `seat`: round 1, this seat's own turn, the
/// once-per-match mulligan unspent, and the seat untouched — nothing of its own
/// on the board, no Glimpse taken, and its anima still exactly the opening income
/// (so a ritual/unwriting that spent anima without a footprint also closes it).
/// 1v1 only (see [`decide_mulligan`]).
pub(crate) fn mulligan_window(state: &GameState, seat: Seat) -> bool {
    if state.is_2v2() {
        return false;
    }
    if state.round != 1 || state.active != seat || state.mulliganed[seat as usize] {
        return false;
    }
    let p = state.player(seat);
    if p.glimpsed_this_turn {
        return false;
    }
    // The opening income (Flow at the seat's first start_turn). Untouched ⇒ the
    // seat has spent nothing.
    let opening_income = (1 + state.round).min(6);
    if p.anima != opening_income {
        return false;
    }
    // No footprint of this seat's own on the board (no Played spirit, no terrain).
    let has_footprint = state.board.iter().any(|t| {
        t.spirit
            .as_ref()
            .map(|sp| sp.owner == seat)
            .unwrap_or(false)
            || t.terrain
                .as_ref()
                .map(|tr| tr.owner == seat)
                .unwrap_or(false)
    });
    !has_footprint
}

fn decide_banish_stray(state: &GameState, ctx: &mut TurnCtx) -> Result<Vec<Event>, Reject> {
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let Some(stray) = state.stray.clone() else {
        return Err(Reject::NothingPending);
    };
    if stray.veiled {
        return Err(Reject::BadTile);
    } // can't banish the unseen
    // Feral Strays are befriendable only at Echo; banishing one always
    // legal. Banishing leaves the banisher's impression.
    push(
        &mut sim,
        &mut evs,
        Event::StrayBanished {
            tile: stray.tile,
            impression: actor,
        },
    );
    Ok(evs)
}

fn decide_reclaim(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Reclaim { tile } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    if (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    let sp = match state.spirit_at(*tile) {
        Some(sp) if sp.owner == actor && !sp.fading => sp.clone(),
        Some(_) => return Err(Reject::NotYourSpirit),
        None => return Err(Reject::TileEmpty),
    };
    // The catalog is immutable behind Arc, so a borrow suffices (no CardDef clone): the
    // name + cost we read live as long as the catalog, independent of the &mut sim mutation.
    let def = card(&ctx.catalog, sp.card);
    // Its Parting fires (it leaves the match deliberately).
    fire_doctrine(
        &mut sim,
        &mut evs,
        &ctx.catalog,
        &def.name,
        crate::effects::Trigger::Parting,
        Some(*tile),
        actor,
    );
    // Reclaim regains ⌊cost/2⌋ Anima — FULL for a spirit whose Parting
    // reclaims full (Last-Light Koi's PartingReclaimsFullCost).
    let reclaims_full = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(&def.name))
        .into_iter()
        .flatten()
        .filter(|s| s.trigger == crate::effects::Trigger::Parting)
        .flat_map(|s| &s.clauses)
        .any(|cl| {
            matches!(
                &cl.effect,
                crate::effects::Effect::Exception(
                    crate::effects::RuleException::PartingReclaimsFullCost
                )
            )
        });
    let mut amount = if reclaims_full {
        def.cost
    } else {
        def.cost / 2
    };
    // Ferrier of the Salt Road: while present, the owner's Fade reclaim regains +1.
    if exception_active(
        &sim,
        &ctx.catalog,
        actor,
        crate::effects::RuleException::FadeReclaimsExtraAnima,
    ) {
        amount += 1;
    }
    push(&mut sim, &mut evs, Event::SpiritReclaimed { tile: *tile });
    if amount > 0 {
        push(
            &mut sim,
            &mut evs,
            Event::AnimaGained {
                seat: actor,
                amount,
                reason: AnimaReason::Effect,
            },
        );
    }
    Ok(evs)
}

fn decide_evolve(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Evolve {
        tile,
        form_hand,
        fuel,
        engage,
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
    if (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    if let Some(d) = fuel
        && (*d as usize) >= state.board.len()
    {
        return Err(Reject::BadTile);
    }
    if let Some(t) = engage
        && (*t as usize) >= state.board.len()
    {
        return Err(Reject::BadTile);
    }
    // Evolve a line-base you own. The base-state ↔ form-type pairing is strict
    // (enforced below once we know the form): a Primal is a Fading base's own
    // last becoming; a Fabled is a HEALTHY base's leap, fueled by a donor.
    let base = match state.spirit_at(*tile) {
        Some(sp) if sp.owner == actor => sp.clone(),
        Some(_) => return Err(Reject::TargetNotEnemy),
        None => return Err(Reject::TileEmpty),
    };
    // Borrow the base's def (no clone): all its reads land before the first &mut ctx
    // mutation (full_exchange et al.), and the catalog is immutable behind Arc.
    let base_def = card(&ctx.catalog, base.card);
    // The form is a CARD IN HAND you play onto the base. Read it at `form_hand`
    // and validate it is a form whose base is THIS base. You cannot evolve a base
    // you hold no form card for.
    let hand = &state.player_slot(actor_slot).hand;
    let Some(&form_id) = hand.get(*form_hand as usize) else {
        return Err(Reject::BadHandIndex);
    };
    // The form's scalars are copied out (and its name cloned) so nothing borrows the
    // catalog across the later &mut ctx engage path, where the form's name is still read.
    let form = card(&ctx.catalog, form_id);
    if form.evolves_from.as_deref() != Some(base_def.name.as_str()) {
        return Err(Reject::EvolveConditionUnmet); // this form is not this base's becoming
    }
    let form_name = form.name.clone();
    let form_reach = form.reach;
    let form_id_val = form.id;
    let (form_attack, form_defense, form_hp) = (form.attack, form.defense, form.hp);
    let form_cost = form.cost;
    let is_fabled = form.rarity == "Fabled";
    // The shared-Imprint rule still gates which forms are legal (honoring the
    // RuleException carriers: Bearer of Small Stones, Duckling, the Shrine). Reuse
    // `legal_evolutions` so those exceptions apply exactly as before; the form
    // must be among the base's legal becomings.
    if !legal_evolutions(state, &ctx.catalog, base_def, actor).contains(&form_id) {
        return Err(Reject::EvolveConditionUnmet);
    }
    // Fuel law: a Fabled form draws on ANOTHER spirit's faded energy —
    // it requires a donor (standing OR fading ally, not the base). A
    // Primal is self-fueled by the base's own Fading and takes no donor.
    // Strict base-state ↔ form-type pairing.
    if is_fabled {
        // Fabled ← a HEALTHY base, the turn AFTER it arrived (reuse the
        // summoning-sickness gate exactly as decide_move_spirit does).
        if base.fading || state.moved_this_turn.contains(tile) {
            return Err(Reject::EvolveConditionUnmet);
        }
    } else {
        // Primal ← a FADING base (its self-fueled last becoming).
        if !base.fading {
            return Err(Reject::EvolveConditionUnmet);
        }
    }
    // Evolution charges the form's cost, half-crediting the base's invested
    // value: `form.cost − ⌊base.cost/2⌋`, then cost-aura adjusted (a cost
    // aura can still shift it), floored at 0. Charged on BOTH paths.
    let eff_cost = (form_cost as i16 - (base_def.cost as i16) / 2
        + cost_aura(state, actor, &ctx.catalog))
    .max(0) as u8;
    if state.player_slot(actor_slot).anima < eff_cost {
        return Err(Reject::NotEnoughAnima);
    }
    if is_fabled {
        let Some(donor_tile) = *fuel else {
            return Err(Reject::BadTile);
        };
        if donor_tile == *tile {
            return Err(Reject::BadTile);
        }
        let donor = match state.spirit_at(donor_tile) {
            Some(sp) if sp.owner == actor && !sp.is_token => sp.clone(),
            Some(_) => return Err(Reject::TargetNotEnemy),
            None => return Err(Reject::TileEmpty),
        };
        // The donor dissolves — its Parting fires; it leaves the
        // owner's impression (spent in service, still remembered). A borrow suffices:
        // the name is read immediately, before any &mut ctx engage path.
        let donor_def = card(&ctx.catalog, donor.card);
        fire_doctrine(
            &mut sim,
            &mut evs,
            &ctx.catalog,
            &donor_def.name,
            crate::effects::Trigger::Parting,
            Some(donor_tile),
            actor,
        );
        push(
            &mut sim,
            &mut evs,
            Event::SpiritDissolved {
                tile: donor_tile,
                impression: donor.banished_by.unwrap_or(actor),
            },
        );
    } else if fuel.is_some() {
        return Err(Reject::BadTile); // a Primal takes no donor
    } else {
        // Primal: the base's own Fading is the fuel — its Parting fires.
        fire_doctrine(
            &mut sim,
            &mut evs,
            &ctx.catalog,
            &base_def.name,
            crate::effects::Trigger::Parting,
            Some(*tile),
            actor,
        );
    }
    // Validate the arrival strike against the FORM's reach.
    if let Some(target) = engage {
        let reach = targeting_reach(state, &ctx.catalog, form_reach, *tile, actor, state.board_w);
        if !reach.contains(target) {
            return Err(Reject::TargetNotInReach);
        }
        match sim.spirit_at(*target) {
            Some(sp) if sp.owner != actor && !sp.fading => {}
            Some(_) => return Err(Reject::TargetNotEnemy),
            None => return Err(Reject::TileEmpty),
        }
    }
    // Pay the discounted cost — `form.cost − ⌊base.cost/2⌋` (cost-aura adjusted),
    // charged on both the Primal and Fabled paths.
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
    // No impression for the base: it BECOMES the form (the played form card lands on
    // it, the same essence transformed — a becoming, not a death). The form card is
    // consumed from hand in `evolve`. The Fabled donor's impression is laid above, at
    // its dissolution.
    push(
        &mut sim,
        &mut evs,
        Event::SpiritEvolved {
            seat: actor,
            tile: *tile,
            from: base.card,
            to: form_id_val,
            attack: form_attack,
            defense: form_defense,
            hp: form_hp,
            // §5.4 Throughline-completion lifecycle, by tier: a FABLED form (a healthy
            // base's continuation) KEEPS the base's `throughline_done`; a PRIMAL (a Fading
            // base's fresh becoming) does not — it arrives re-completable. The reducer
            // applies this without the catalog.
            keeps_throughline: is_fabled,
        },
    );
    // Matron of the Long Goodbye: spirits you evolve arrive +10/+10.
    if exception_active(
        &sim,
        &ctx.catalog,
        actor,
        crate::effects::RuleException::EvolveArrivesBuffed,
    ) {
        push(
            &mut sim,
            &mut evs,
            Event::EffectStat {
                tile: *tile,
                attack: 10,
                defense: 10,
                form: 0,
            },
        );
    }
    // Evolution is an arrival: the new form may strike, may be
    // intercepted, and may chain Momentum — exactly like a Call.
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
    interception(&mut sim, &mut evs, ctx, *tile, actor);
    if banished {
        momentum(&mut sim, &mut evs, ctx, *tile, actor);
    }
    // Again!: an evolution arrival may also take a queued second engage.
    offer_second_engage(&mut sim, &mut evs, &ctx.catalog, actor, *tile, *engage);
    check_throughline(&mut sim, &mut evs, &ctx.catalog, *tile, actor);
    if sim.spirit_at(*tile).map(|sp| !sp.fading).unwrap_or(false) {
        fire_effects(
            &mut sim,
            &mut evs,
            ctx,
            crate::effects::Trigger::OnPlay,
            &form_name,
            Some(*tile),
            actor,
        );
    }
    Ok(evs)
}

/// Devolution (design §5) — recede a standing-Faded **form** to a **base** the seat
/// holds in hand, the rescue. The strict rules:
/// - **target** (`tile`): a form YOU own that is **standing-Faded** — `fading` AND
///   `fade_deadline` Some (a Primal/Fabled banished in combat, still inside its §0.5
///   window). An uncontested fade (deadline None) or a healthy spirit is not eligible;
///   neither is a non-form (a base has no tier to recede to).
/// - **the played card** (`base_hand`): a hand card that is a **base in the form's
///   line** — its name is the form's `evolves_from` (lines are 2-stage, no chains, so
///   the form's base is its direct `evolves_from`).
/// - **cost**: HALF the banished form's Anima, **rounded down** (`form.cost / 2`).
/// - **an arrival, symmetric with evolution** (the maintainer's ruling: *if evolutions
///   are arrivals, devolutions should be too*): after the recede lands it fires the SAME
///   arrival triggers a form's evolution fires — `apply_next_arrival` (a queued
///   Kindle/Again! buff lands on the base) and `check_throughline` (a base receding into a
///   standing 3-line **re-completes on the spot** — +10/+10 and a full heal, at parity
///   with a Primal-evolve into a line). It still **engages no one** (the recede carries no
///   strike target — no `full_exchange`, `momentum`, or second engage) and fires **no
///   OnPlay**, and the base is still summoning-sick (no free Mobile step the turn it
///   devolves — exactly as an evolved form is).
/// - the base **replaces** the form at **full HP**, fade cleared (rescued one tier
///   down), and is **summoning-sick** (`evolve` marks `moved_this_turn`).
///
/// Emits a distinct `SpiritDevolved` (the base card consumed from hand in `evolve`).
fn decide_devolve(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Devolve { tile, base_hand } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let actor_slot = state.active_slot;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    if (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    // The target is a STANDING-FADED form you own: fading + a combat fade_deadline.
    let form = match state.spirit_at(*tile) {
        Some(sp) if sp.owner == actor => sp.clone(),
        Some(_) => return Err(Reject::NotYourSpirit),
        None => return Err(Reject::TileEmpty),
    };
    // It must be inside the standing-Faded window — banished in combat, still standing.
    if !form.fading || form.fade_deadline.is_none() {
        return Err(Reject::DevolveConditionUnmet);
    }
    // Borrow the form/base defs (no clone): every read lands before the &mut sim arrival
    // steps, and the catalog is immutable behind Arc.
    let form_def = card(&ctx.catalog, form.card);
    // Only a FORM recedes — a base has no tier below it.
    let Some(base_name) = form_def.evolves_from.as_deref() else {
        return Err(Reject::DevolveConditionUnmet);
    };
    // The played card is a BASE in this form's line — a hand card whose name is the
    // form's direct base (lines are 2-stage; the no-chain lock makes this exact).
    let hand = &state.player_slot(actor_slot).hand;
    let Some(&base_id) = hand.get(*base_hand as usize) else {
        return Err(Reject::BadHandIndex);
    };
    let base = card(&ctx.catalog, base_id);
    if base.name != base_name {
        return Err(Reject::DevolveConditionUnmet); // not the form's base
    }
    // Cost: HALF the banished form's Anima, rounded down. (No cost-aura — the rescue
    // is priced off the form it recedes from, per §5.)
    let eff_cost = form_def.cost / 2;
    if state.player_slot(actor_slot).anima < eff_cost {
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
    }
    // The base replaces the form at FULL HP, fade cleared, summoning-sick. `evolve`
    // consumes the base card from hand and marks `moved_this_turn` (summoning sickness).
    push(
        &mut sim,
        &mut evs,
        Event::SpiritDevolved {
            seat: actor,
            tile: *tile,
            from: form.card,
            to: base.id,
            attack: base.attack,
            defense: base.defense,
            hp: base.hp,
        },
    );
    // Devolution is an arrival, symmetric with evolution (the maintainer's ruling): the
    // rescued base fires the same arrival triggers a form's evolution fires. It carries no
    // engage target, so the engage-gated steps (full_exchange / momentum / second engage)
    // and OnPlay are deliberately NOT run — devolution engages no one and fires no OnPlay
    // (design §5). What DOES fire, exactly as in `decide_evolve`:
    //   1. `apply_next_arrival` — a queued Kindle/Again! next-arrival buff lands on the base.
    //   2. `check_throughline` — a base receding into a standing 3-line re-completes the
    //      Throughline on the spot (+10/+10 + full heal), at parity with the evolve-into-
    //      a-line case. (`SpiritDevolved` reset `throughline_done` to false, so a base that
    //      had completed before the fade is eligible to complete anew here.)
    apply_next_arrival(&mut sim, &mut evs, actor, *tile);
    check_throughline(&mut sim, &mut evs, &ctx.catalog, *tile, actor);
    Ok(evs)
}

fn decide_reveal(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Reveal { tile, engage } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    if state.pending_choice.is_some() {
        return Err(Reject::ChoicePending);
    }
    if (*tile as usize) >= state.board.len() {
        return Err(Reject::BadTile);
    }
    if let Some(t) = engage
        && (*t as usize) >= state.board.len()
    {
        return Err(Reject::BadTile);
    }
    let sp = match state.spirit_at(*tile) {
        Some(sp) if sp.owner == actor && sp.face_down && !sp.fading => sp.clone(),
        Some(_) => return Err(Reject::TargetNotEnemy),
        None => return Err(Reject::TileEmpty),
    };
    // Copy the reach scalar + clone only the name (read after the &mut ctx engage path),
    // rather than cloning the whole CardDef; the catalog is immutable behind Arc.
    let def_reach = card(&ctx.catalog, sp.card).reach;
    let def_name = card(&ctx.catalog, sp.card).name.clone();
    if let Some(target) = engage {
        // Don't Look: a restricted lurker may still step into the light,
        // but can't strike on the reveal.
        if sp.no_engage_until >= state.round {
            return Err(Reject::EngageRestricted);
        }
        let reach = targeting_reach(state, &ctx.catalog, def_reach, *tile, actor, state.board_w);
        if !reach.contains(target) {
            return Err(Reject::TargetNotInReach);
        }
        match state.spirit_at(*target) {
            Some(e) if e.owner != actor && !e.fading => {}
            Some(_) => return Err(Reject::TargetNotEnemy),
            None => return Err(Reject::TileEmpty),
        }
    }
    push(&mut sim, &mut evs, Event::SpiritRevealed { tile: *tile });
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
    // The Arrival Law in full: a reveal is an arrival.
    interception(&mut sim, &mut evs, ctx, *tile, actor);
    if banished {
        momentum(&mut sim, &mut evs, ctx, *tile, actor);
    }
    if sim.spirit_at(*tile).map(|sp| !sp.fading).unwrap_or(false) {
        fire_effects(
            &mut sim,
            &mut evs,
            ctx,
            crate::effects::Trigger::OnReveal,
            &def_name,
            Some(*tile),
            actor,
        );
    }
    Ok(evs)
}

fn decide_set_orders(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::SetOrders { tile, hold } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    // Free action: standing orders cost no action.
    match state.spirit_at(*tile) {
        Some(sp) if sp.owner == actor && !sp.fading => {}
        _ => return Err(Reject::TileEmpty),
    }
    push(
        &mut sim,
        &mut evs,
        Event::OrdersSet {
            tile: *tile,
            hold: *hold,
        },
    );
    Ok(evs)
}

fn decide_choose(
    state: &GameState,
    cmd: &Command,
    ctx: &mut TurnCtx,
) -> Result<Vec<Event>, Reject> {
    let Command::Choose { index } = cmd else {
        unreachable!()
    };
    let mut sim = state.clone();
    let mut evs = Vec::new();
    let actor = ctx.actor;
    let Some(pending) = state.pending_choice.clone() else {
        return Err(Reject::NothingPending);
    };
    match pending {
        crate::state::PendingChoice::Peek { seat, looked } => {
            if seat != actor {
                return Err(Reject::NotYourTurn);
            }
            if *index as usize >= looked.len() {
                return Err(Reject::BadHandIndex);
            }
            push(
                &mut sim,
                &mut evs,
                Event::PeekTaken {
                    seat,
                    index: *index,
                },
            );
        }
        crate::state::PendingChoice::GlimpseBurn { seat, burnable } => {
            if seat != actor {
                return Err(Reject::NotYourTurn);
            }
            // Glimpse step 1 — the BURN cost. The chosen hand card is spent (leaves
            // play); `evolve` removes it. Then peek the top and open the keep-or-
            // bottom `Glimpse` (the burn didn't touch the deck, so the top is
            // unchanged from the glimpse). A non-empty page was a precondition of
            // offering the glimpse at all, so the top is present here.
            if *index as usize >= burnable.len() {
                return Err(Reject::BadHandIndex);
            }
            push(
                &mut sim,
                &mut evs,
                Event::GlimpseBurned {
                    seat,
                    hand_index: *index,
                },
            );
            // Peek the ACTIVE slot's top (per-slot pages in 2v2). The burn
            // didn't touch the deck, so this is the same top the glimpse will offer.
            let top = sim
                .player_slot(sim.active_slot)
                .deck
                .first()
                .copied()
                .expect("a non-empty page was a precondition of offering the glimpse");
            push(
                &mut sim,
                &mut evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Glimpse { seat, top },
                },
            );
        }
        crate::state::PendingChoice::Glimpse { seat, .. } => {
            if seat != actor {
                return Err(Reject::NotYourTurn);
            }
            // Glimpse step 2 — two options only: 0 = KEEP (stay on top, no Anima),
            // 1 = BOTTOM (+1 Anima). The card itself rides the pending choice
            // (`top`); `evolve` moves it on BOTTOM, so the resolution event carries
            // only the verdict.
            let kept = match *index {
                0 => true,
                1 => false,
                _ => return Err(Reject::BadHandIndex),
            };
            push(&mut sim, &mut evs, Event::GlimpseResolved { seat, kept });
            if !kept {
                // Bottoming the draw buys focus — the +1 Anima it grants has a cost
                // (the card you let go).
                push(
                    &mut sim,
                    &mut evs,
                    Event::AnimaGained {
                        seat,
                        amount: 1,
                        reason: AnimaReason::Glimpse,
                    },
                );
            }
        }
        crate::state::PendingChoice::Target {
            seat,
            options,
            effect,
            source,
        } => {
            if seat != actor {
                return Err(Reject::NotYourTurn);
            }
            let Some(&tile) = options.get(*index as usize) else {
                return Err(Reject::BadTile);
            };
            push(&mut sim, &mut evs, Event::TargetChosen { tile });
            apply_choice_effect(&mut sim, &mut evs, ctx, effect, source, tile, actor);
            // Otterling Magus: a ritual's armed extra targets — apply the same effect
            // to that many more options (deterministic, skipping the chosen tile).
            let extra = sim.ritual_extra_targets;
            if extra > 0 {
                push(&mut sim, &mut evs, Event::RitualExtraTargetsConsumed);
                let mut applied = 0u8;
                for &opt in options.iter() {
                    if applied >= extra {
                        break;
                    }
                    if opt == tile {
                        continue;
                    }
                    apply_choice_effect(&mut sim, &mut evs, ctx, effect, source, opt, actor);
                    applied += 1;
                }
            }
        }
        crate::state::PendingChoice::Recover { seat, options } => {
            if seat != actor {
                return Err(Reject::NotYourTurn);
            }
            let Some(&card) = options.get(*index as usize) else {
                return Err(Reject::BadHandIndex);
            };
            push(&mut sim, &mut evs, Event::RecoverTaken { seat, card });
        }
    }
    Ok(evs)
}
