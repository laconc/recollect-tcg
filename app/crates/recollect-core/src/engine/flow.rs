//! Turn flow: **Flow → Main → Fade** (Main is the `Phase::Acting` state — the design
//! renamed the player-facing phase Act→Main). `start_turn` opens the Flow (income, draw,
//! orphan/bond upkeep); `end_turn` runs the Fade (the turn-END dissolve of a
//! combat-banished base, §0.5) then the seat/round/Dusk wrap. There is no turn-START
//! fade, and the Dusk is instant (it dissolves its rim Unwritten in the `MemoryContracted`
//! apply). Also: anima income, round advance, finish/scoring.
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

/// One full simultaneous exchange (engage or chain link): both strikes read
/// pre-damage stats; Echo rolls resolve inside — before any chain decision.
/// Returns true if the defender was banished.
/// A full combat exchange between an attacker at one tile and a defender at
/// another: both deal simultaneous damage (resonance edges, Echo variance for
/// the eligible side, Arcane/Warded via `eff_defense`); a defeated spirit is
/// banished (banisher's impression) or replaced. Returns whether the target was
/// defeated (so the caller can run `momentum`).
/// Pack Tactics: if the spirit at `att_tile` is in a Bond whose GrantEngage carries a
/// `pre_chip` (and the pair is present & adjacent and the PARTNER has `def_tile` in its
/// reach), the partner chips that much damage to the target first. Returns the chip.
/// Held Ground: vacated lingering rim ground fades behind its keeper.
pub(crate) fn fade_if_held(sim: &mut GameState, evs: &mut Vec<Event>, tile: u8) {
    if sim.contracted
        && is_rim_w(tile, sim.board_w)
        && !sim.board[tile as usize].faded
        && sim.board[tile as usize].spirit.is_none()
    {
        push(sim, evs, Event::TileFaded { tile });
    }
}

/// A Bond breaks the instant either bonded spirit leaves play — banished, dissolved,
/// **or replaced by an enemy** (an Overwrite that takes the tile) — or the pair drifts
/// apart (pushes break Bonds, the Steadfast rule). A Bond is a same-owner construct
/// (`AttachBond` requires both endpoints be your own standing spirits, §-Bonds), so an
/// endpoint that no longer holds a STANDING spirit **owned by the bond's owner** has
/// broken it: checking ownership — not just presence — is what fells a bond the moment an
/// Overwrite steals one of its tiles, before a stale enemy bond can redirect a chain blow
/// onto the overwriter that took the tile. Idempotent: emits a `BondBroken` per newly
/// broken bond and no-ops once they are gone, so it is safe to call mid-resolution
/// (after a banish/Overwrite) and again at the Flow.
pub(crate) fn prune_broken_bonds(sim: &mut GameState, evs: &mut Vec<Event>, catalog: &[CardDef]) {
    let broken: Vec<CardId> = sim
        .bonds
        .iter()
        .filter(|b| {
            // Each endpoint must still hold a standing spirit owned by the bond's owner;
            // an empty/fading tile — or one an enemy now stands on — has broken the bond.
            let held = |t: u8| {
                sim.spirit_at(t)
                    .map(|s| !s.fading && s.owner == b.owner)
                    .unwrap_or(false)
            };
            // Unbreakable survives a single 1-tile separation (manhattan ≤ 2); a normal
            // Bond needs the pair adjacent (manhattan == 1).
            let max_dist = if card_carries_static_exception(
                catalog,
                b.card,
                crate::effects::RuleException::BondHoldsUnlessSeparated,
            ) {
                2
            } else {
                1
            };
            !held(b.tile_a) || !held(b.tile_b) || manhattan(b.tile_a, b.tile_b) > max_dist
        })
        .map(|b| b.card)
        .collect();
    for card in broken {
        push(sim, evs, Event::BondBroken { card });
    }
}

/// Momentum with no preference list (the auto-heuristic) — used by every
/// arrival path except the player's own PlaySpirit, which may carry prefs.
pub(crate) fn momentum(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    tile: u8,
    actor: Seat,
) {
    momentum_prefs(sim, evs, ctx, tile, actor, &[]);
}

/// Kindle: apply the seat's queued next-arrival buff to the just-arrived spirit at
/// `tile` (before its engage), then clear it. A no-op if none is queued. Called at
/// each arrival point (Play, Evolve).
pub(crate) fn apply_next_arrival(sim: &mut GameState, evs: &mut Vec<Event>, actor: Seat, tile: u8) {
    let atk = sim.next_arrival_atk[actor as usize];
    if atk != 0 {
        let until = sim.round;
        push(
            sim,
            evs,
            Event::EffectTempStat {
                tile,
                attack: atk,
                defense: 0,
                until_round: until,
            },
        );
        push(sim, evs, Event::NextArrivalConsumed { seat: actor });
    }
}

/// Again!: after the next arrival's first engage, offer it a SECOND engage against a
/// chosen enemy in its reach (a Target choice resolving to EngageFrom). Consumes the
/// offer either way. `first` is the arrival's first engage target (excluded).
pub(crate) fn offer_second_engage(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    actor: Seat,
    tile: u8,
    first: Option<u8>,
) {
    let Some(penalty) = sim.next_arrival_2nd_engage[actor as usize] else {
        return;
    };
    push(sim, evs, Event::NextArrivalSecondConsumed { seat: actor });
    let Some(sp) = sim.spirit_at(tile) else {
        return;
    };
    if sp.fading {
        return;
    }
    let reach = targeting_reach(
        sim,
        catalog,
        card(catalog, sp.card).reach,
        tile,
        actor,
        sim.board_w,
    );
    let options: Vec<u8> = reach
        .into_iter()
        .filter(|&t| {
            Some(t) != first
                && sim
                    .spirit_at(t)
                    .map(|s| s.owner != actor && !s.fading)
                    .unwrap_or(false)
        })
        .collect();
    if !options.is_empty() {
        push(
            sim,
            evs,
            Event::ChoiceOffered {
                choice: crate::state::PendingChoice::Target {
                    seat: actor,
                    options,
                    effect: crate::state::ChoiceEffect::EngageFrom {
                        from: tile,
                        bonus: penalty,
                    },
                    source: tile,
                },
            },
        );
    }
}

/// `EndTurn` runs the turn's tail: the **Fade phase** (now at turn-END, after Main —
/// the turn is **Flow → Main → Fade**), then orphan-token sweep, bond breaks, the
/// round/Dusk wrap, and the next seat's Flow.
///
/// **Fade** = the standing-Faded window closing: a base `actor` owns that was banished
/// in combat lingered standing-Faded through this Main (its one chance to **evolve or
/// devolve** it); if it is still fading at this turn-END and its deadline has come
/// (`round >= fade_deadline`), it dissolves now (firing Partings + OnAnyBanish, laying
/// the banisher's impression). Done BEFORE the round advance / finish so the deadline
/// reads the turn's own round. A base banished on `actor`'s OWN turn carries
/// `fade_deadline = round + 1`, so it is NOT yet due — it survives to the owner's NEXT
/// turn-end (a full Main later). With the Dusk now instant (it dissolves its rim
/// Unwritten at the contraction, never deferring a fade), EVERY fading spirit carries a
/// `fade_deadline`, so this is the sole Fade step — there is no turn-START fade.
pub(crate) fn end_turn(sim: &mut GameState, evs: &mut Vec<Event>, actor: Seat, ctx: &mut TurnCtx) {
    push(sim, evs, Event::TurnEnded { seat: actor });
    let due: Vec<(u8, Seat)> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit.as_ref().and_then(|sp| match sp.fade_deadline {
                Some(deadline) if sp.owner == actor && sp.fading && sim.round >= deadline => {
                    Some((i as u8, sp.banished_by.unwrap_or(sp.owner)))
                }
                _ => None,
            })
        })
        .collect();
    for (tile, impression) in due {
        // Hold the Memory: a delayed base skips THIS Fade (the skip is spent) and
        // lingers to the next turn-end — extending its standing-Faded window by a turn.
        // Its Parting waits with it. The skip MUST be spent via an event
        // (`FadeDelayConsumed`): the Fade runs on `decide`'s clone, so a bare
        // `fade_delayed.swap_remove` here is lost on the committed board — the tile would
        // stay delayed every turn and the base would NEVER dissolve (an immortal fading
        // body). With the event journaled, the next due turn-end finds it no longer delayed
        // and dissolves it.
        if sim.fade_delayed.contains(&tile) {
            push(sim, evs, Event::FadeDelayConsumed { tile });
            continue;
        }
        dissolve_faded_at(sim, evs, &ctx.catalog, tile, impression);
    }
    if actor == Seat::B {
        let old = sim.round;
        if old == sim.rules.contraction_after {
            let faded: Vec<u8> = (0..sim.board.len() as u8)
                .filter(|t| {
                    // The Held Ground law: at the Curl, EMPTY rim fades — and so do Unwritten, who
                    // get no Held Ground (the Solace's hold on the margin is swept, while a player's
                    // standing spirit lingers and keeps scoring).
                    is_rim_w(*t, sim.board_w)
                        && !sim.board[*t as usize].faded
                        && match &sim.board[*t as usize].spirit {
                            None => true,
                            Some(sp) => ctx
                                .catalog
                                .iter()
                                .find(|c| c.id == sp.card)
                                .is_some_and(|c| c.kind.is_antagonist_creature()),
                        }
                })
                .collect();
            push(sim, evs, Event::MemoryContracted { faded_tiles: faded });
        }
        if old >= sim.rules.last_round {
            finish(sim, evs, &ctx.catalog);
            return;
        }
        push(sim, evs, Event::RoundAdvanced { round: old + 1 });
    }
    start_turn(sim, evs, &ctx.catalog);
    // The Memory may stir at a turn's start (seeded; inner tiles only).
    stray_surfacing(sim, evs, ctx);
}

/// The standing-Faded window: the round by the end of which a spirit banished
/// in combat (at the given `round`, on the given `active` seat's turn) must dissolve,
/// expressed as the round of its `owner`'s **next turn-end**. A pure, deterministic
/// function of public state (no entropy, never in a view) — the rounds line up
/// because a round runs A's turn then B's, the round advancing after B:
/// - the owner banished on their OWN turn (`owner == active`) gets their NEXT turn,
///   a full round later → `round + 1` (so the dissolve skips the current turn's end);
/// - banished on the opponent's turn, owner **A** (B is acting) → A acts next round,
///   so `round + 1`;
/// - banished on the opponent's turn, owner **B** (A is acting) → B acts later THIS
///   round, so `round` (the deadline is the imminent B turn-end).
///
/// Checked at the owner's turn-end (`end_turn`) as `round >= fade_deadline`.
pub(crate) fn fade_deadline_round(round: u8, active: Seat, owner: Seat) -> u8 {
    if owner == active || owner == Seat::A {
        round + 1
    } else {
        round
    }
}

/// Dissolve one Fading spirit at `tile`, firing its Parting (and any bonded
/// endpoint's Parting), an Unwritten's OnUnwrite, then leaving the right mark:
/// nothing for an Unwritten, an erasure-tally `ImpressionForgotten` next to a
/// Lacuna, or the banisher's impression (`SpiritDissolved`, unwritten by The Long
/// Rest if adjacent). Closes with `fade_if_held` + the OnAnyBanish broadcast. The
/// single dissolution body shared by the turn-START Fade step (uncontested fades)
/// and the turn-END lingering dissolve (combat fades). (On round 12 a banished
/// base lingers standing-Faded like any other base — `banish_or_replace` no longer
/// dissolves it on defeat — and `finish`'s bare final pass dissolves it before
/// scoring, no Parting/OnAnyBanish.) `impression` is the banisher's color
/// (`banished_by`, else the owner).
pub(crate) fn dissolve_faded_at(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    tile: u8,
    impression: Seat,
) {
    // Parting fires while the teller still stands fading (Wisp's hush).
    // The Unwritten leave NO impression — the lore-stated rule, keyed on the
    // CardKind itself (Unwritten | IllIntent), so it catches every Unwritten
    // without over-suppressing Kindred.
    let is_unwritten = sim
        .spirit_at(tile)
        .map(|s| card(catalog, s.card).is_unwritten())
        .unwrap_or(false);
    if let Some(sp) = sim.spirit_at(tile) {
        let (name, owner) = (card(catalog, sp.card).name.clone(), sp.owner);
        fire_doctrine(
            sim,
            evs,
            catalog,
            &name,
            crate::effects::Trigger::Parting,
            Some(tile),
            owner,
        );
        // A bonded endpoint Parting also fires its BOND's Parting, so the bond
        // can act on the loss (The Long Walk heals the surviving partner).
        if let Some(bcard) = sim
            .bonds
            .iter()
            .find(|b| b.tile_a == tile || b.tile_b == tile)
            .map(|b| b.card)
        {
            let bname = card(catalog, bcard).name.clone();
            fire_doctrine(
                sim,
                evs,
                catalog,
                &bname,
                crate::effects::Trigger::Parting,
                Some(tile),
                owner,
            );
        }
    }
    if is_unwritten {
        // Footnote / Sentence Fragment: when an Unwritten Unwrites (dissolves), its
        // OnUnwrite fires while it still stands — before it leaves nothing behind.
        if let Some(sp) = sim.spirit_at(tile) {
            let (uname, uowner) = (card(catalog, sp.card).name.clone(), sp.owner);
            fire_doctrine(
                sim,
                evs,
                catalog,
                &uname,
                crate::effects::Trigger::OnUnwrite,
                Some(tile),
                uowner,
            );
        }
        // The Unwritten leave nothing — not even an impression. Emit an EVENT so the
        // removal reaches the committed state: `decide` runs on a CLONE and only the
        // pushed events are replayed by `evolve`, so a bare `sim.board[..].spirit = None`
        // here is lost on the real board (the Unwritten would never actually leave —
        // staying standing-Faded and re-firing its dissolution every turn). `TokenDissolved`
        // removes the spirit leaving no impression (the Unwritten IS a token).
        if sim.spirit_at(tile).is_some() {
            push(sim, evs, Event::TokenDissolved { tile });
        }
    } else if impression == Seat::A && lacuna_denies_impression(sim, catalog, tile) {
        // A Lacuna nearby denies the PLAYER's foothold — the spirit dissolves but no
        // impression forms next to the Solace's hole in the page. Keeping the player off the
        // tile is itself an erasure, so it SCORES: `ImpressionForgotten` tallies it. (Only the
        // player's mark, impression == A, is denied here; a Solace banish, B, falls through to
        // the normal dissolve, where lay_mark tallies it instead.) The spirit removal must
        // RIDE AN EVENT (the decide-clone/evolve-replay split, as above): `SpiritReleased`
        // takes the body off leaving no impression, then `ImpressionForgotten` banks the
        // erasure tally. (`ImpressionForgotten` only clears+tallies the mark; it does not
        // remove the spirit, so both events are needed.)
        push(sim, evs, Event::SpiritReleased { tile });
        push(sim, evs, Event::ImpressionForgotten { tile });
    } else {
        push(sim, evs, Event::SpiritDissolved { tile, impression });
        // The Long Rest: a spirit dissolving adjacent to it leaves no impression (the dissolve
        // still ticks; only the impression is unwritten).
        if adjacent_denies_impression(sim, catalog, tile) {
            push(sim, evs, Event::ImpressionUnwritten { tile });
        }
    }
    fade_if_held(sim, evs, tile);
    // Faultline-class terrain: a Landmark whose OnAnyBanish fires when a spirit fully
    // dissolves ON it ("dissolves here"). Terrain is not a standing spirit, so the
    // OnAnyBanish witness broadcast below would never reach it — fire it explicitly here,
    // where the dissolving `tile` (= the Landmark's tile) is known.
    faultline_dissolve_here(sim, evs, catalog, tile);
    // OnAnyBanish to every standing witness.
    dissolution_effects(sim, evs, catalog, None);
}

/// The start of a turn (the **Flow** opens here): sweep orphan tokens, break stretched
/// Bonds, then grant income + the draw, and run the Stray surfacing pass. The same for
/// every seat — the Solace (seat B) has no special start path.
///
/// **There is no turn-START Fade step.** The Fade phase moved to turn-END (after Main,
/// in `end_turn`); and the Dusk is now instant (it dissolves its rim Unwritten at the
/// contraction, in the `MemoryContracted` apply). So no fading spirit is ever waiting to
/// be processed here — every Fading spirit is a combat-banished base inside its
/// standing-Faded window, dissolved at its owner's turn-END.
pub(crate) fn start_turn(sim: &mut GameState, evs: &mut Vec<Event>, catalog: &[CardDef]) {
    let seat = sim.active;
    // Any Kindred whose caller has left play fades — to no impression.
    sweep_orphan_tokens(sim, evs, catalog);
    // A Bond breaks if either spirit has left, an enemy took its tile, or the pair drifted
    // apart (pushes break Bonds — the Steadfast rule). The same prune runs mid-resolution
    // after an Overwrite, so a stale enemy bond can't reach across a stolen tile.
    prune_broken_bonds(sim, evs, catalog);
    // Flow: income, then the draw; the hand cap opens the Release window.
    let income = (1 + sim.round).min(6);
    push(
        sim,
        evs,
        Event::AnimaGained {
            seat,
            amount: income,
            reason: AnimaReason::Income,
        },
    );
    // Patience: pay any anima this seat was promised "at your next Flow".
    if sim.pending_flow_anima[seat as usize] > 0 {
        push(sim, evs, Event::FlowAnimaPaid { seat });
    }
    // AtFlow: persistent sources owned by the active seat tithe/heal at the Flow.
    fire_at_flow(sim, evs, catalog, seat);
    if !sim.player(seat).deck.is_empty() {
        push(sim, evs, Event::CardDrawn { seat });
        if sim.player(seat).hand.len() > MAX_HAND {
            push(sim, evs, Event::ReleaseRequired { seat });
        }
    }
}

/// Patience: a played card may promise anima "at your next Flow" — an
/// AtFlow/OncePerMatch/Owner/AnimaDelta clause that `fire_at_flow` (which only fires
/// on-board persistent sources) never sees. Scheduled here at play time; paid out at
/// the owner's next income step (`FlowAnimaPaid`).
pub(crate) fn schedule_flow_anima(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    owner: Seat,
) {
    use crate::effects::{Condition, Effect as E, Selector as S, Trigger};
    let _ = catalog;
    let amount: i16 = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(card_name))
        .into_iter()
        .flatten()
        .filter(|s| s.trigger == Trigger::AtFlow && s.condition == Condition::OncePerMatch)
        .flat_map(|s| &s.clauses)
        .filter_map(|cl| match (&cl.selector, &cl.effect) {
            (S::Owner, E::AnimaDelta { amount }) if *amount > 0 => Some(*amount as i16),
            _ => None,
        })
        .sum();
    if amount > 0 {
        push(
            sim,
            evs,
            Event::FlowAnimaScheduled {
                seat: owner,
                amount: amount as u8,
            },
        );
    }
}

/// Fire the active seat's persistent AtFlow effects (its Landmarks and Bonds) at
/// its Flow: a Landmark's anima tithe (Wellspring) or occupant heal (Hearth), and
/// a Bond's pair heal (Shared Umbrella). Reads are collected before any push to
/// keep the board borrow and the event application apart.
pub(crate) fn fire_at_flow(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    seat: Seat,
) {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let atflow = |card_id: CardId| -> Vec<crate::effects::Clause> {
        crate::effects::specs_for(catalog, card_id)
            .into_iter()
            .flatten()
            .filter(|s| s.trigger == Trigger::AtFlow)
            .flat_map(|s| s.clauses.iter().cloned())
            .collect()
    };
    let mut anima = 0i16;
    let mut restores: Vec<(u8, i16)> = Vec::new();
    for i in 0..sim.board.len() as u8 {
        if let Some(terr) = &sim.board[i as usize].terrain
            && !terr.face_down
            && terr.owner == seat
        {
            for cl in atflow(terr.card) {
                match (&cl.selector, &cl.effect) {
                    (S::Owner, E::AnimaDelta { amount }) => anima += *amount as i16,
                    (S::OccupantHere, E::RestoreForm { amount })
                        if sim.spirit_at(i).map(|s| !s.fading).unwrap_or(false) =>
                    {
                        restores.push((i, *amount));
                    }
                    _ => {}
                }
            }
        }
    }
    for b in &sim.bonds {
        if b.owner != seat
            || manhattan(b.tile_a, b.tile_b) != 1
            || !sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
            || !sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
        {
            continue;
        }
        for cl in atflow(b.card) {
            if let (S::BondedPair, E::RestoreForm { amount }) = (&cl.selector, &cl.effect) {
                restores.push((b.tile_a, *amount));
                restores.push((b.tile_b, *amount));
            }
        }
    }
    // Elder of the Unbroken Watch: a standing spirit with an AtFlow heal-adjacent clause
    // restores its adjacent allies at the owner's Flow.
    for i in 0..sim.board.len() as u8 {
        let Some(sp) = sim.spirit_at(i) else { continue };
        if sp.owner != seat || sp.fading || sp.face_down {
            continue;
        }
        for cl in atflow(sp.card) {
            if let (S::AdjacentAlliesAll, E::RestoreForm { amount }) = (&cl.selector, &cl.effect) {
                for j in 0..sim.board.len() as u8 {
                    if j != i
                        && manhattan(i, j) == 1
                        && sim
                            .spirit_at(j)
                            .map(|s| s.owner == seat && !s.fading)
                            .unwrap_or(false)
                    {
                        restores.push((j, *amount));
                    }
                }
            }
        }
    }
    // A standing spirit's OWN-state AtFlow effects — resolved deterministically at its
    // owner's Flow (no choice; the spirit acts on itself or its own tile):
    //   * SelfSpirit/RestoreForm — Quiet Tide ("heals at end of round").
    //   * SelfSpirit/GrowEachFlow — Foal Born During the Storm ("+5/+5 each Flow, capped").
    //   * Owner/ImpressionRemoveTarget — The Last Warm Page ("one ADJACENT impression
    //     fades"): the Solace erodes a single enemy mark next to the page (gentle erosion).
    // Each is scanned and applied here; without this, these clauses were inert (the Foal
    // never grew, Quiet Tide never healed, the page eroded nothing).
    let mut grows: Vec<(u8, i16, i16)> = Vec::new(); // (tile, atk_step, def_step)
    let mut warm_pages: Vec<u8> = Vec::new(); // Last Warm Page tiles
    for i in 0..sim.board.len() as u8 {
        let Some(sp) = sim.spirit_at(i) else { continue };
        if sp.owner != seat || sp.fading || sp.face_down {
            continue;
        }
        for cl in atflow(sp.card) {
            match (&cl.selector, &cl.effect) {
                (S::SelfSpirit, E::RestoreForm { amount }) => restores.push((i, *amount)),
                (S::SelfSpirit, E::GrowEachFlow { step, max }) => {
                    // Grow up to `max` TOTAL above printed stats; bank only the remaining room
                    // (so it stops at the cap, never overshoots — "max +20/+20").
                    let printed = card(catalog, sp.card);
                    let astep = (*step).min((*max - (sp.attack - printed.attack)).max(0));
                    let dstep = (*step).min((*max - (sp.defense - printed.defense)).max(0));
                    if astep > 0 || dstep > 0 {
                        grows.push((i, astep, dstep));
                    }
                }
                (S::Owner, E::ImpressionRemoveTarget) => warm_pages.push(i),
                _ => {}
            }
        }
    }
    for (tile, astep, dstep) in grows {
        push(
            sim,
            evs,
            Event::EffectStat {
                tile,
                attack: astep,
                defense: dstep,
                form: 0,
            },
        );
    }
    // The Last Warm Page: erase ONE enemy impression adjacent to the page. The Solace
    // forgetting an existing mark scores (ImpressionForgotten, +1 tally); a non-Solace
    // owner just clears it. Lowest tile index for determinism (no entropy is consulted).
    let solace = sim.rules.factions[seat as usize] == crate::types::Faction::Solace;
    for page in warm_pages {
        if let Some(mark) = adjacent4_w(page, sim.board_w)
            .find(|&a| sim.board[a as usize].impressions.contains(&seat.other()))
        {
            if solace {
                push(sim, evs, Event::ImpressionForgotten { tile: mark });
            } else {
                push(sim, evs, Event::ImpressionUnwritten { tile: mark });
            }
        }
    }
    // Sage of And-Then: a standing spirit with an AtFlow Glimpse opens it at the owner's
    // Flow (the owner is active during their own Flow, so the interactive PeekDeck is safe).
    let sages: Vec<(u8, crate::effects::Clause)> = (0..sim.board.len() as u8)
        .filter_map(|i| {
            let sp = sim.spirit_at(i)?;
            if sp.owner != seat || sp.fading || sp.face_down {
                return None;
            }
            let cl = atflow(sp.card)
                .into_iter()
                .find(|cl| matches!(cl.effect, E::PeekDeck { .. }))?;
            Some((i, cl))
        })
        .collect();
    for (i, cl) in sages {
        exec_clause_mode(
            sim,
            evs,
            catalog,
            &cl,
            Some(i),
            seat,
            None,
            ChoiceMode::Open,
            None,
            None,
        );
    }
    // The Unfiled: a standing spirit with an OnAttuned spec draws each Flow WHILE it is
    // attuned (its AdjacentAlliesShareResonance condition holds now). Ruling: the "When
    // Attuned" benefit is a per-turn engine, not a one-shot transition.
    let mut attuned_draws = 0usize;
    for i in 0..sim.board.len() as u8 {
        let Some(sp) = sim.spirit_at(i) else { continue };
        if sp.owner != seat || sp.fading || sp.face_down {
            continue;
        }
        let Some(specs) = crate::effects::specs_for(catalog, sp.card) else {
            continue;
        };
        // Precise (catalog-aware) attunement: 2+ adjacent allies of the SAME Resonance.
        let attuned = specs.iter().any(|s| {
            if let crate::effects::Condition::AdjacentAlliesShareResonance { n } = s.condition {
                shared_adjacent_resonance(sim, catalog, i, sp, n).is_some()
            } else {
                false
            }
        });
        if !attuned {
            continue;
        }
        for s in specs.iter().filter(|s| s.trigger == Trigger::OnAttuned) {
            for cl in &s.clauses {
                if let (S::Owner, E::Draw { count }) = (&cl.selector, &cl.effect) {
                    attuned_draws += *count as usize;
                }
            }
        }
    }
    for _ in 0..attuned_draws {
        if !sim.player(seat).deck.is_empty() {
            push(sim, evs, Event::CardDrawn { seat });
        }
    }
    if anima > 0 {
        push(
            sim,
            evs,
            Event::AnimaGained {
                seat,
                amount: anima as u8,
                reason: crate::state::AnimaReason::Effect,
            },
        );
    }
    for (tile, amount) in restores {
        push(sim, evs, Event::EffectRestored { tile, amount });
    }
}

/// Erasure's Patience: the set of tiles whose impressions DO NOT score at Nightfall —
/// every tile orthogonally adjacent to a standing spirit carrying the
/// `AdjacentImpressionsDontScore` exception (the marks near it cooled). Owner-agnostic:
/// the text reads "adjacent impressions," so it cools whoever's mark sits beside it.
fn erasure_patience_cooled_tiles(sim: &GameState, catalog: &[CardDef]) -> Vec<u8> {
    use crate::effects::RuleException::AdjacentImpressionsDontScore;
    let mut cooled = Vec::new();
    for i in 0..sim.board.len() as u8 {
        if sim.board[i as usize].spirit.as_ref().is_some_and(|sp| {
            !sp.fading
                && card_carries_static_exception(catalog, sp.card, AdjacentImpressionsDontScore)
        }) {
            cooled.extend(adjacent4_w(i, sim.board_w));
        }
    }
    cooled
}

/// End the match: tally Score (each side's standing spirits + impressions under
/// the Held Ground law) and record the final `Phase::Finished` with scores. This is
/// the **Nightfall step**: a spirit **banished on round 12** lingered standing-Faded
/// through the rest of the round (§0.5 — no Main left to evolve in, but a body that
/// stands until the telling ends) and dissolves HERE, before scoring, laying the
/// banisher's impression so the OPPONENT scores the tile.
pub(crate) fn finish(sim: &mut GameState, evs: &mut Vec<Event>, catalog: &[CardDef]) {
    // Resolve every remaining fading spirit (the round-12 lingering bases among
    // them), laying the banisher's color, THEN score — never the reverse.
    let fading: Vec<(u8, Seat)> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit.as_ref().and_then(|sp| {
                if sp.fading {
                    Some((i as u8, sp.banished_by.unwrap_or(sp.owner)))
                } else {
                    None
                }
            })
        })
        .collect();
    for (tile, impression) in fading {
        // No effects at the final dissolve — the telling is over. An UNWRITTEN leaves
        // NOTHING even at Nightfall (§11: "a player who banishes an Unwritten gets
        // nothing — it dissolves leaving no mark of having been"); a bare `SpiritDissolved`
        // here would `lay_mark` the banisher's color, wrongly scoring the Unwritten's tile
        // for a Lorekeeper banisher. `TokenDissolved` removes it leaving no impression and
        // no tally (it never *was*), matching the turn-END `dissolve_faded_at`.
        let is_unwritten = sim
            .spirit_at(tile)
            .map(|s| card(catalog, s.card).is_unwritten())
            .unwrap_or(false);
        if is_unwritten {
            push(sim, evs, Event::TokenDissolved { tile });
        } else {
            push(sim, evs, Event::SpiritDissolved { tile, impression });
        }
        fade_if_held(sim, evs, tile);
    }
    // Let It Lie (Unwriting): on its round, impressions do not score (the grief is set down).
    let impressions_score = sim.impressions_dormant_round != Some(sim.round);
    // Erasure's Patience (Unwritten): impressions on tiles adjacent to it do not score
    // (the marks near it cool one by one). Precompute the cooled tiles once.
    let cooled = erasure_patience_cooled_tiles(sim, catalog);
    let mut a = 0u8;
    let mut b = 0u8;
    for (i, t) in sim.board.iter().enumerate() {
        // One point per tile, to whatever is last on it: the standing spirit if present, else the
        // most-recent impression (a new banish overwrites the old mark). No stacking, no overlap.
        if let Some(sp) = &t.spirit {
            match sp.owner {
                Seat::A => a += 1,
                Seat::B => b += 1,
            }
        } else if let Some(&s) = t.impressions.first()
            && impressions_score
            && !cooled.contains(&(i as u8))
        {
            match s {
                Seat::A => a += 1,
                Seat::B => b += 1,
            }
        }
    }
    // The Solace's off-board erasure tally joins its board score (seat B is the Solace).
    b = b.saturating_add(sim.solace_erasures);
    let result = if a > b {
        MatchResult::Win(Seat::A)
    } else if b > a {
        MatchResult::Win(Seat::B)
    } else {
        MatchResult::Draw
    };
    push(
        sim,
        evs,
        Event::MatchEnded {
            result,
            score_a: a,
            score_b: b,
        },
    );
}
