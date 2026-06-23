//! The per-phase effect handlers `exec_clause_mode` dispatches to (choice-routing, target-spirit, copy/control/strip/etc.).
//! A sibling of `effects_exec.rs`; `use super::*` shares helpers.
use super::*;

pub(crate) fn exec_adjacent_ally_choose(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    mode: ChoiceMode,
) {
    use crate::effects::{Duration, Effect as E, Selector as S};
    let Some(src) = source else { return };
    // Doctrine-resolved free-target effects: resolve to the doctrine pick in ANY mode,
    // never an interactive choice. These fire from contexts where opening a PendingChoice
    // is wrong — an arrival auto-effect the Solace doesn't deliberate (What You Set Down's
    // Bounce, The Kind Erasure's Release), or a COMBAT trigger that must resolve inline
    // (The Unsaid Cruelty's OnDefeat Damage — opening a cross-turn window mid-exchange would
    // strand combat). Without this, an OnDefeat/OnEngage `AdjacentEnemyChoose/Damage` hit the
    // Open-mode arm's `_ => return` and silently did nothing (The Unsaid Cruelty was dead).
    if matches!(
        cl.effect,
        E::Bounce | E::Damage { .. } | E::Release | E::Banish
    ) {
        if let Some(t) = doctrine_pick(sim, &cl.selector, &cl.effect, src, owner) {
            apply_clause_at(sim, evs, cl, t);
            // Lethal effect-damage must dissolve the target (laying the source-owner's
            // impression), like the board-wide Damage executor — otherwise a ≤0-HP spirit
            // would stand, breaking the invariant. A 10-chip rarely kills, but a wounded
            // target must fall.
            if matches!(cl.effect, E::Damage { .. })
                && sim
                    .spirit_at(t)
                    .map(|sp| sp.hp <= 0 && !sp.fading)
                    .unwrap_or(false)
            {
                banish_or_replace(sim, evs, catalog, t, owner);
            }
        }
        return;
    }
    match mode {
        ChoiceMode::Doctrine => {
            if let Some(t) = doctrine_pick(sim, &cl.selector, &cl.effect, src, owner) {
                apply_clause_at(sim, evs, cl, t);
            }
        }
        ChoiceMode::Open => {
            let want_ally = matches!(cl.selector, S::AdjacentAllyChoose);
            let options: Vec<u8> = sim
                .board
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    t.spirit
                        .as_ref()
                        .filter(|sp| {
                            !sp.fading
                                && (sp.owner == owner) == want_ally
                                && manhattan(src, i as u8) == 1
                        })
                        .map(|_| i as u8)
                })
                .collect();
            if options.is_empty() {
                return;
            }
            let effect = match &cl.effect {
                E::Displace(crate::effects::Displacement::Push { tiles }) => {
                    crate::state::ChoiceEffect::PushAway { tiles: *tiles }
                }
                E::RestoreForm { amount } => {
                    crate::state::ChoiceEffect::RestoreForm { amount: *amount }
                }
                E::StatDelta {
                    attack,
                    defense,
                    form,
                } => crate::state::ChoiceEffect::StatDelta {
                    attack: *attack,
                    defense: *defense,
                    form: *form,
                    this_round: cl.duration == Duration::ThisRound,
                },
                _ => return,
            };
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect,
                        source: src,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_reveal_fabrication(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    owner: Seat,
    mode: ChoiceMode,
) {
    if mode == ChoiceMode::Open {
        let options: Vec<u8> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.terrain
                    .as_ref()
                    .filter(|tr| {
                        tr.face_down
                            && tr.kind == crate::state::TerrainKind::Fabrication
                            && tr.owner != owner
                    })
                    .map(|_| i as u8)
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
                        effect: crate::state::ChoiceEffect::PeekFabrication,
                        source: 0,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_reveal_fabrication_2(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    owner: Seat,
    mode: ChoiceMode,
) {
    if mode == ChoiceMode::Open {
        let options: Vec<u8> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.terrain
                    .as_ref()
                    .filter(|tr| tr.face_down && tr.kind == crate::state::TerrainKind::Fabrication)
                    .map(|_| i as u8)
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
                        effect: crate::state::ChoiceEffect::RevealFabricationAt,
                        source: 0,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_target_fading_spirit(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    owner: Seat,
    mode: ChoiceMode,
) {
    if mode == ChoiceMode::Open {
        let options: Vec<u8> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.spirit.as_ref().filter(|sp| sp.fading).map(|_| i as u8))
            .collect();
        if !options.is_empty() {
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect: crate::state::ChoiceEffect::DelayFade,
                        source: 0,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_target_spirit(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    mode: ChoiceMode,
    cost_cap: Option<u8>,
) {
    use crate::effects::{Duration, Effect as E, Selector as S};
    let want = |sp: &Spirit| match cl.selector {
        S::TargetAllySpirit => sp.owner == owner,
        S::TargetEnemySpirit => sp.owner != owner,
        _ => true,
    };
    let options: Vec<u8> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit
                .as_ref()
                .filter(|sp| {
                    !sp.fading
                        && want(sp)
                        // What's-Its-Name: unnameable, so no Ritual/target effect may aim at it.
                        && !ritual_untargetable(catalog, sp.card)
                        && cost_cap
                            .map(|cap| card(catalog, sp.card).cost <= cap)
                            .unwrap_or(true)
                })
                .map(|_| i as u8)
        })
        .collect();
    if options.is_empty() {
        return;
    }
    let effect = match &cl.effect {
        E::StatDelta {
            attack,
            defense,
            form,
        } => crate::state::ChoiceEffect::StatDelta {
            attack: *attack,
            defense: *defense,
            form: *form,
            this_round: cl.duration == Duration::ThisRound,
        },
        E::RestoreForm { amount } => crate::state::ChoiceEffect::RestoreForm { amount: *amount },
        E::Damage { amount } => crate::state::ChoiceEffect::Damage { amount: *amount },
        E::GrantKeyword { keyword } => crate::state::ChoiceEffect::GrantKeyword {
            keyword: *keyword,
            until_round: if cl.duration == Duration::ThisRound {
                sim.round
            } else {
                u8::MAX
            },
        },
        // Two-step displacement (Quiet Step, Misstep, Cold Spot): step 1 picks
        // the spirit; resolving it opens the destination choice.
        E::Displace(
            crate::effects::Displacement::MoveAny { tiles }
            | crate::effects::Displacement::Push { tiles },
        ) => crate::state::ChoiceEffect::DisplaceFrom { tiles: *tiles },
        // Behind You: step 1 picks one enemy; resolving opens the adjacent-enemy choice.
        E::Displace(crate::effects::Displacement::Swap) => crate::state::ChoiceEffect::SwapStep1,
        // The Fog of Elsewhere: return the chosen (Cost-capped) enemy to hand.
        E::Bounce => crate::state::ChoiceEffect::Bounce,
        // Don't Look: the chosen enemy can't engage or intercept (NextRound =
        // through the end of the next round).
        E::Restrict(crate::effects::Restriction::Engage) => {
            crate::state::ChoiceEffect::RestrictEngage {
                until_round: match cl.duration {
                    Duration::NextRound => sim.round + 1,
                    _ => sim.round,
                },
            }
        }
        // Tailwind: a per-spirit FULL reach buff this round.
        E::ReachDelta {
            forward,
            all_directions,
            ..
        } => crate::state::ChoiceEffect::ReachBuff {
            forward: *forward,
            all_directions: *all_directions,
        },
        // Reckless Charge: the chosen ally engages now (a dependent target choice).
        E::GrantEngage {
            immediate: true, ..
        } => crate::state::ChoiceEffect::EngageStep1 { bonus: 0 },
        // Reckless Charge: the chosen ally's retaliation shifts this round.
        E::RetaliationDelta { amount } => {
            crate::state::ChoiceEffect::RetaliateThisRound { delta: *amount }
        }
        _ => return,
    };
    match mode {
        ChoiceMode::Open => push(
            sim,
            evs,
            Event::ChoiceOffered {
                choice: crate::state::PendingChoice::Target {
                    seat: owner,
                    options,
                    effect,
                    source: source.unwrap_or(0),
                },
            },
        ),
        // Off-turn (Parting etc.): the dying don't deliberate — strongest target.
        ChoiceMode::Doctrine => {
            if let Some(t) = options
                .iter()
                .copied()
                .max_by_key(|t| sim.spirit_at(*t).map(|s| s.attack).unwrap_or(0))
            {
                apply_clause_at(sim, evs, cl, t);
            }
        }
    }
}

pub(crate) fn exec_target_two_adjacent_allies(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    owner: Seat,
    mode: ChoiceMode,
) {
    use crate::effects::{Duration, Effect as E};
    if let E::StatDelta {
        attack, defense, ..
    } = &cl.effect
    {
        let options: Vec<u8> = (0..sim.board.len() as u8)
            .filter(|&i| {
                sim.spirit_at(i)
                    .map(|s| !s.fading && s.owner == owner)
                    .unwrap_or(false)
                    && (0..sim.board.len() as u8).any(|j| {
                        j != i
                            && manhattan(i, j) == 1
                            && sim
                                .spirit_at(j)
                                .map(|s| !s.fading && s.owner == owner)
                                .unwrap_or(false)
                    })
            })
            .collect();
        if mode == ChoiceMode::Open && !options.is_empty() {
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect: crate::state::ChoiceEffect::PairBuffStep1 {
                            attack: *attack,
                            defense: *defense,
                            this_round: cl.duration == Duration::ThisRound,
                        },
                        source: 0,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_target_bonded_pair(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    owner: Seat,
    mode: ChoiceMode,
) {
    use crate::effects::Effect as E;
    if let E::RestoreForm { amount } = &cl.effect {
        let options: Vec<u8> = sim
            .bonds
            .iter()
            .filter(|b| {
                b.owner == owner
                    && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
                    && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
            })
            .map(|b| b.tile_a)
            .collect();
        if mode == ChoiceMode::Open && !options.is_empty() {
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect: crate::state::ChoiceEffect::HealBondedPair { amount: *amount },
                        source: 0,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_bounce(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) {
    use crate::effects::Selector as S;
    if matches!(cl.selector, S::Engager) {
        for t in effect_targets(sim, &cl.selector, source, owner, engager) {
            if sim.spirit_at(t).is_some() {
                push(sim, evs, Event::SpiritBounced { tile: t });
            }
        }
    }
}

pub(crate) fn exec_copy_last_played(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    source: Option<u8>,
    owner: Seat,
) {
    if let Some(src) = source
        && let Some(lpc) = sim.last_played_spirit[owner as usize]
    {
        let d = card(catalog, lpc);
        push(
            sim,
            evs,
            Event::SpiritCopiedStats {
                tile: src,
                attack: d.attack,
                defense: d.defense,
                hp_max: d.hp,
            },
        );
    }
}

pub(crate) fn exec_copy_engager_reach(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    source: Option<u8>,
    engager: Option<u8>,
) {
    if let Some(src) = source
        && let Some(eng) = engager
        && let Some(esp) = sim.spirit_at(eng)
    {
        let reach = card(catalog, esp.card).reach;
        push(sim, evs, Event::ReachCopied { tile: src, reach });
    }
}

/// The Misremembered: copy the PRINTED Attack/Defense of the spirit it just fought
/// (the `engager` bound by the OnEngageResolved fire). It keeps its own HP/form — only
/// A/D change — so we re-emit its current `hp_max` unchanged through SpiritCopiedStats.
pub(crate) fn exec_copy_printed_stats(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    source: Option<u8>,
    engager: Option<u8>,
) {
    if let Some(src) = source
        && let Some(opp) = engager
        && let Some(osp) = sim.spirit_at(opp)
        && let Some(me) = sim.spirit_at(src)
    {
        let d = card(catalog, osp.card);
        push(
            sim,
            evs,
            Event::SpiritCopiedStats {
                tile: src,
                attack: d.attack,
                defense: d.defense,
                hp_max: me.hp_max,
            },
        );
    }
}

pub(crate) fn exec_take_control(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) {
    use crate::effects::Selector as S;
    if matches!(cl.selector, S::Engager) {
        for t in effect_targets(sim, &cl.selector, source, owner, engager) {
            if sim.spirit_at(t).is_some() {
                push(
                    sim,
                    evs,
                    Event::ControlTaken {
                        tile: t,
                        new_owner: owner,
                    },
                );
            }
        }
    }
}

pub(crate) fn exec_impression_remove_target(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    owner: Seat,
    mode: ChoiceMode,
) {
    use crate::effects::Selector as S;
    if cl.selector == S::Owner && mode == ChoiceMode::Open {
        let options: Vec<u8> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| (t.impressions.contains(&owner.other())).then_some(i as u8))
            .collect();
        if !options.is_empty() {
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect: crate::state::ChoiceEffect::RemoveImpression,
                        source: 0,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_trait_strip(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) {
    use crate::effects::{Duration, Selector as S};
    // The Blank Page (Engager, permanent) vs. Smear (EngagedEnemy, this round): both blank
    // the target's printed Keywords/Traits, but the duration differs.
    if matches!(cl.selector, S::Engager | S::EngagedEnemy) {
        for t in effect_targets(sim, &cl.selector, source, owner, engager) {
            if sim.spirit_at(t).is_some() {
                if cl.duration == Duration::ThisRound {
                    push(
                        sim,
                        evs,
                        Event::TraitsStrippedUntil {
                            tile: t,
                            until_round: sim.round,
                        },
                    );
                } else {
                    push(sim, evs, Event::TraitsStripped { tile: t });
                }
            }
        }
    }
}

pub(crate) fn exec_recover(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    owner: Seat,
    mode: ChoiceMode,
) {
    use crate::effects::Selector as S;
    if cl.selector == S::Owner && mode == ChoiceMode::Open {
        let options: Vec<CardId> = sim
            .dissolved
            .iter()
            .filter(|(s, _)| *s == owner)
            .map(|(_, c)| *c)
            .collect();
        if !options.is_empty() {
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Recover {
                        seat: owner,
                        options,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_self_spirit(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    source: Option<u8>,
    owner: Seat,
    mode: ChoiceMode,
    pay_hp: Option<i16>,
) {
    if let (Some(hp), Some(src)) = (pay_hp, source)
        && mode == ChoiceMode::Open
        && let Some(sp) = sim.spirit_at(src)
    {
        let reach = targeting_reach(
            sim,
            catalog,
            card(catalog, sp.card).reach,
            src,
            owner,
            sim.board_w,
        );
        let mut options: Vec<u8> = reach
            .into_iter()
            .filter(|&t| {
                sim.spirit_at(t)
                    .map(|s| s.owner != owner && !s.fading)
                    .unwrap_or(false)
            })
            .collect();
        if !options.is_empty() {
            options.push(src); // choosing yourself DECLINES (the "may pay")
            push(
                sim,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Target {
                        seat: owner,
                        options,
                        effect: crate::state::ChoiceEffect::PayEngage {
                            from: src,
                            hp,
                            bonus: 0,
                        },
                        source: src,
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_re_trigger_parting(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    source: Option<u8>,
    owner: Seat,
    mode: ChoiceMode,
) {
    if mode == ChoiceMode::Open {
        let has_parting = |c: crate::types::CardId| {
            crate::effects::specs_for(catalog, c)
                .map(|specs| {
                    specs
                        .iter()
                        .any(|s| s.trigger == crate::effects::Trigger::Parting)
                })
                .unwrap_or(false)
        };
        let options: Vec<u8> = sim
            .board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.spirit
                    .as_ref()
                    .filter(|sp| {
                        sp.owner == owner && !sp.fading && !sp.face_down && has_parting(sp.card)
                    })
                    .map(|_| i as u8)
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
                        effect: crate::state::ChoiceEffect::ReTriggerParting,
                        source: source.unwrap_or(0),
                    },
                },
            );
        }
    }
}

pub(crate) fn exec_next_arrival_this_turn(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    owner: Seat,
) {
    use crate::effects::Effect as E;
    match &cl.effect {
        E::StatDelta { attack, .. } => push(
            sim,
            evs,
            Event::NextArrivalBuffed {
                seat: owner,
                attack: *attack,
            },
        ),
        E::SecondTargetEngage { attack_penalty } => push(
            sim,
            evs,
            Event::NextArrivalSecondEngage {
                seat: owner,
                penalty: *attack_penalty,
            },
        ),
        _ => {}
    }
}

pub(crate) fn exec_exception(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    cl: &crate::effects::Clause,
    owner: Seat,
) {
    use crate::effects::Selector as S;
    if cl.selector == S::Owner {
        push(sim, evs, Event::EvolveImprintFreed { seat: owner });
    }
}
