//! The fire_* dispatch + target resolvers (release/restore), spring_fabrication, dissolution, and the suppression predicates.
//! A sibling of `effects_exec.rs`; `use super::*` shares helpers.
use super::*;

pub(crate) fn fire_effects(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    trigger: crate::effects::Trigger,
    card_name: &str,
    source: Option<u8>,
    owner: Seat,
) {
    fire_effects_noctx(sim, evs, &ctx.catalog, card_name, trigger, source, owner);
}

pub(crate) fn fire_effects_noctx(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    trigger: crate::effects::Trigger,
    source: Option<u8>,
    owner: Seat,
) {
    fire_with_engager(sim, evs, catalog, card_name, trigger, source, owner, None);
}

/// On-engaged effects: the defender's OnEngageResolved with the engager bound.
pub(crate) fn fire_engaged(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    tile: u8,
    owner: Seat,
    engager: u8,
) {
    fire_with_engager(
        sim,
        evs,
        catalog,
        card_name,
        crate::effects::Trigger::OnEngageResolved,
        Some(tile),
        owner,
        Some(engager),
    );
}

/// Follow-up: spring a face-down Fabrication. An enemy spirit (the
/// `engager`) has stepped into the lie at `tile`. The Fabrication reveals and
/// its OnReveal clauses fire with the engager bound — Traps punish the engager
/// (Damage/Bounce/TakeControl…), Bluffs reward the owner (Draw/Anima…). The
/// terrain is consumed (a sprung lie is spent), leaving the tile open.
pub(crate) fn spring_fabrication(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    tile: u8,
    engager: Option<u8>,
) {
    let catalog = &ctx.catalog.clone();
    let Some(terr) = sim.board[tile as usize].terrain.clone() else {
        return;
    };
    if terr.kind != crate::state::TerrainKind::Fabrication || !terr.face_down {
        return;
    }
    let name = card(catalog, terr.card).name.clone();
    push(sim, evs, Event::FabricationRevealed { tile });
    buff_on_fab_reveal(sim, evs, catalog);
    // OnReveal: Traps bind Engager, Bluffs bind Owner (both handled by selectors).
    fire_with_engager(
        sim,
        evs,
        catalog,
        &name,
        crate::effects::Trigger::OnReveal,
        Some(tile),
        terr.owner,
        engager,
    );
    // The Provocation: the engager is FORCED into one more engage at −10 — a doctrine
    // target (the trap owner's strongest spirit in the engager's reach), if any.
    let provokes = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(&name))
        .into_iter()
        .flatten()
        .filter(|s| s.trigger == crate::effects::Trigger::OnReveal)
        .flat_map(|s| &s.clauses)
        .any(|cl| {
            cl.selector == crate::effects::Selector::Engager
                && matches!(cl.effect, crate::effects::Effect::ExtraEngage)
        });
    if provokes
        && let Some(eng) = engager
        && let Some(esp) = sim.spirit_at(eng)
        && !esp.fading
    {
        let reach = targeting_reach(
            sim,
            catalog,
            card(catalog, esp.card).reach,
            eng,
            esp.owner,
            sim.board_w,
        );
        let target = reach
            .into_iter()
            .filter(|&t| {
                sim.spirit_at(t)
                    .map(|s| s.owner == terr.owner && !s.fading)
                    .unwrap_or(false)
            })
            .max_by_key(|&t| sim.spirit_at(t).map(|s| s.attack).unwrap_or(0));
        if let Some(tgt) = target {
            full_exchange(sim, evs, ctx, eng, tgt, StrikeKind::Engage, -10);
        }
    }
    // The lie is spent — unless the clause turned it into a Landmark (those
    // specs flip the terrain kind via their own effect; if it's still a
    // face-up Fabrication, it dissolves to nothing).
    if let Some(t) = sim.board[tile as usize].terrain.as_ref()
        && t.kind == crate::state::TerrainKind::Fabrication
    {
        sim.board[tile as usize].terrain = None;
        push(sim, evs, Event::FabricationSpent { tile });
    }
}

// Fire a card's authored effect clauses for a given `trigger` (OnPlay,
// Parting, OnReveal, Static…), binding the `engager` so Engager/Owner
// selectors resolve. The data-driven effect path: clauses come from
// `effects.json`, interpreted here. `fire_effects`/`fire_doctrine` are thin
// wrappers choosing the choice-handling mode.

/// Target resolution for RestoreForm specifically: like `effect_targets` but
/// FADING-INCLUSIVE for ally selectors, because a restore is meant to be able
/// to save a dissolving ally (a heal that excluded the dying would be useless
/// on exactly the spirits that need it). Non-ally selectors fall back to the
/// normal resolver.
/// Targets for Release (the Solace's mercy): **FADING-ONLY** and any-owner.
/// The card text is "release every adjacent *fading* spirit" — the mercy spares
/// the dying, never the living, so a healthy spirit in scope is skipped (the
/// aggressive "banish a healthy enemy, no impression" line is
/// [`crate::effects::Effect::Banish`] → [`banish_targets`], which does NOT filter
/// on fading). Selectors map to adjacency / board scope. Unlike restore_targets
/// (allies only) and effect_targets (skips fading + owner-scoped), this reaches
/// dying spirits of either side.
pub(crate) fn release_targets(
    sim: &GameState,
    sel: &crate::effects::Selector,
    source: Option<u8>,
    owner: Seat,
) -> Vec<u8> {
    targets_in_scope(sim, sel, source, owner, /* fading_only */ true)
}

/// Targets for Banish (the IllIntent erasure): any owner, **any state** (healthy
/// included) — You Were Never Really Here / I Too Can Create Desolation banish a
/// living enemy outright, leaving no impression. Same scope resolution as
/// [`release_targets`] but without the fading filter.
pub(crate) fn banish_targets(
    sim: &GameState,
    sel: &crate::effects::Selector,
    source: Option<u8>,
    owner: Seat,
) -> Vec<u8> {
    targets_in_scope(sim, sel, source, owner, /* fading_only */ false)
}

/// The shared scope resolver for the no-impression removal effects (Release =
/// mercy, Banish = erasure). `fading_only` gates whether healthy spirits in
/// scope are included: Release passes `true` (the dying only), Banish `false`.
fn targets_in_scope(
    sim: &GameState,
    sel: &crate::effects::Selector,
    source: Option<u8>,
    owner: Seat,
    fading_only: bool,
) -> Vec<u8> {
    use crate::effects::Selector as S;
    let all = |pred: &dyn Fn(u8, &Spirit) -> bool| -> Vec<u8> {
        sim.board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.spirit.as_ref().map(|sp| (i as u8, sp)))
            .filter(|(i, sp)| (!fading_only || sp.fading) && pred(*i, sp))
            .map(|(i, _)| i)
            .collect()
    };
    let adj = |want_enemy: Option<bool>| -> Vec<u8> {
        let Some(src) = source else { return vec![] };
        all(&|i, sp| {
            if Some(i) == source {
                return false;
            }
            if manhattan(src, i) != 1 {
                return false;
            }
            match want_enemy {
                Some(true) => sp.owner != owner,
                Some(false) => sp.owner == owner,
                None => true,
            }
        })
    };
    match sel {
        S::AdjacentAlliesAll => adj(Some(false)),
        S::AdjacentAllyChoose => adj(Some(false)).into_iter().take(1).collect(),
        S::AdjacentEnemiesAll => adj(Some(true)),
        S::AdjacentEnemyChoose => adj(Some(true)).into_iter().take(1).collect(),
        S::AlliesAll => all(&|_, sp| sp.owner == owner),
        S::EnemiesAll => all(&|_, sp| sp.owner != owner),
        S::AllOtherSpirits => all(&|i, _| Some(i) != source),
        _ => vec![],
    }
}

pub(crate) fn restore_targets(
    sim: &GameState,
    sel: &crate::effects::Selector,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) -> Vec<u8> {
    use crate::effects::Selector as S;
    let ally_incl_fading = |pred: &dyn Fn(u8, &Spirit) -> bool| -> Vec<u8> {
        sim.board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.spirit.as_ref().map(|sp| (i as u8, sp)))
            .filter(|(i, sp)| sp.owner == owner && pred(*i, sp))
            .map(|(i, _)| i)
            .collect()
    };
    match sel {
        S::AlliesAll => ally_incl_fading(&|_, _| true),
        S::AdjacentAlliesAll => {
            let Some(src) = source else { return vec![] };
            ally_incl_fading(&|i, _| Some(i) != source && manhattan(src, i) == 1)
        }
        S::SelfSpirit => source.into_iter().collect(),
        // The other spirit of `source`'s Bond (The Long Walk: heal the survivor when
        // its partner Parts). The Parting fires while the dying spirit still stands,
        // so its bond is still in `sim.bonds`.
        S::BondedPartner => {
            let Some(src) = source else { return vec![] };
            sim.bonds
                .iter()
                .find_map(|b| {
                    if b.tile_a == src {
                        Some(b.tile_b)
                    } else if b.tile_b == src {
                        Some(b.tile_a)
                    } else {
                        None
                    }
                })
                .filter(|&t| sim.spirit_at(t).is_some())
                .into_iter()
                .collect()
        }
        // Other selectors (Engager, choice, etc.) use the standard path.
        _ => effect_targets(sim, sel, source, owner, engager),
    }
}

pub(crate) fn fire_with_engager(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    trigger: crate::effects::Trigger,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) {
    fire_mode(
        sim,
        evs,
        catalog,
        card_name,
        trigger,
        source,
        owner,
        engager,
        ChoiceMode::Open,
    );
}

/// Is a given RuleException active for `seat`? Scans the seat's
/// standing (non-fading, revealed) spirits and owned terrain for a carrier
/// whose Static clause declares that exception. This is the single chokepoint
/// every rule that can be excepted consults — no more hardcoded name-checks.
pub(crate) fn exception_active(
    sim: &GameState,
    catalog: &[CardDef],
    seat: Seat,
    which: crate::effects::RuleException,
) -> bool {
    use crate::effects::{Effect, Trigger};
    let carries = |card_id: CardId| -> bool {
        let Some(specs) = crate::effects::specs_for(catalog, card_id) else {
            return false;
        };
        specs.iter().any(|s| {
            s.trigger == Trigger::Static
                && s.clauses
                    .iter()
                    .any(|cl| matches!(&cl.effect, Effect::Exception(x) if *x == which))
        })
    };
    sim.board.iter().enumerate().any(|(i, t)| {
        let spirit_hit = t
            .spirit
            .as_ref()
            .map(|sp| sp.owner == seat && !sp.fading && !sp.face_down && carries(sp.card))
            .unwrap_or(false);
        // Silence Spreads: a silenced Landmark's text is inert this round.
        let silenced = sim.silenced_terrain == Some((i as u8, sim.round));
        let terrain_hit = !silenced
            && t.terrain
                .as_ref()
                .map(|tr| tr.owner == seat && !tr.face_down && carries(tr.card))
                .unwrap_or(false);
        spirit_hit || terrain_hit
    })
}

pub(crate) fn fire_doctrine(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    trigger: crate::effects::Trigger,
    source: Option<u8>,
    owner: Seat,
) {
    // Hush: a spirit adjacent to a Hush has its Parting suppressed (it cannot say goodbye).
    if trigger == crate::effects::Trigger::Parting
        && let Some(tile) = source
        && hush_suppresses_parting(sim, catalog, tile)
    {
        return;
    }
    fire_mode(
        sim,
        evs,
        catalog,
        card_name,
        trigger,
        source,
        owner,
        None,
        ChoiceMode::Doctrine,
    );
    // PartingTriggersTwice (The Vigilkeeper / Rainpool) — a Parting
    // resolves a second time when the owner carries the exception.
    if trigger == crate::effects::Trigger::Parting
        && exception_active(
            sim,
            catalog,
            owner,
            crate::effects::RuleException::PartingTriggersTwice,
        )
    {
        fire_mode(
            sim,
            evs,
            catalog,
            card_name,
            trigger,
            source,
            owner,
            None,
            ChoiceMode::Doctrine,
        );
    }
    // Elegist Wren: any ally's Parting within a Wren's reach grows the Wren.
    if trigger == crate::effects::Trigger::Parting
        && let Some(parting_tile) = source
    {
        buff_elegist_wrens(sim, evs, catalog, owner, parting_tile);
    }
}

pub(crate) fn fire_mode(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    card_name: &str,
    trigger: crate::effects::Trigger,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
    mode: ChoiceMode,
) {
    use crate::effects::Condition;
    let Some(specs) = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(card_name))
    else {
        return;
    };
    let specs = specs.clone();
    for spec in &specs {
        if spec.trigger != trigger {
            continue;
        }
        // Most fired effects are unconditional; a Bond's fired clauses (Common Cause's
        // OnDefeat) gate on PairAdjacent — the source's bond present and adjacent.
        let cond_ok = match spec.condition {
            Condition::Always => true,
            Condition::PairAdjacent => source
                .map(|src| {
                    sim.bonds.iter().any(|b| {
                        (b.tile_a == src || b.tile_b == src)
                            && manhattan(b.tile_a, b.tile_b) == 1
                            && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
                            && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
                    })
                })
                .unwrap_or(false),
            // Gather In: the bonus clause fires only if the owner controls a Bond.
            Condition::YouControlABond => sim.bonds.iter().any(|b| b.owner == owner),
            // The Fog of Elsewhere: a target cost cap, not a clause gate — fire and
            // let the executor filter eligible targets to Cost ≤ cap.
            Condition::CostAtMost { .. } => true,
            // Ragewoken Bison: a "may pay HP" gate — fire only if the source can
            // afford (HP > amount); the pay itself is offered at the choice.
            Condition::PayForm { amount } => source
                .and_then(|s| sim.spirit_at(s))
                .map(|sp| sp.hp > amount)
                .unwrap_or(false),
            // Tooth in the Margin: "first strike each match deals +20" — an OnPlay
            // OncePerMatch buff. A spirit ARRIVES once, so its arrival IS that single
            // instance; we fire it on the OnPlay (a re-manifested copy is a fresh spirit,
            // a documented simplification of the per-match cap). OncePerMatch on any other
            // fired trigger stays unhandled (false).
            Condition::OncePerMatch if trigger == crate::effects::Trigger::OnPlay => true,
            _ => false,
        };
        let cost_cap = match spec.condition {
            Condition::CostAtMost { cost } => Some(cost),
            _ => None,
        };
        let pay_hp = match spec.condition {
            Condition::PayForm { amount } => Some(amount),
            _ => None,
        };
        if cond_ok {
            for cl in &spec.clauses {
                exec_clause_mode(
                    sim, evs, catalog, cl, source, owner, engager, mode, cost_cap, pay_hp,
                );
            }
        }
    }
}

/// A spirit fully dissolved: its Parting (every authored Parting clause is
/// choice-bound, so nothing fires here), then every standing witness's
/// OnAnyBanish (Saudade, Vesper, Wake-Walker).
/// Lacuna: the Solace's signature denial. While a standing Lacuna (the
/// ill-intent Unwritten named "Lacuna") sits adjacent to `tile`, no Narrator impression
/// may form there — the hole in the page swallows the foothold. Keyed purely on the
/// Lacuna card's presence (it only exists if the Solace played one) — it denies wherever
/// it stands, like every other on-board effect.
pub(crate) fn lacuna_denies_impression(sim: &GameState, catalog: &[CardDef], tile: u8) -> bool {
    adjacent4_w(tile, sim.board_w).any(|a| {
        sim.board[a as usize]
            .spirit
            .as_ref()
            .map(|sp| !sp.fading && card(catalog, sp.card).key == "lacuna")
            .unwrap_or(false)
    })
}

/// The Smudge / Null Choir: the spirit at `tile` is adjacent to an ENEMY whose Static aura
/// carries `TraitSilence` (selector AdjacentEnemiesAll) — so its trait-borne combat value
/// (the Chorus tribal bonus, the only engine-modeled "trait" that grants stats) is blurred
/// to nothing while it stands beside the silence. A read-time suppression, like the Lacuna /
/// Lullaby / Hush predicates: pure over public board state, no event.
pub(crate) fn trait_silenced(sim: &GameState, catalog: &[CardDef], tile: u8) -> bool {
    let Some(me) = sim.spirit_at(tile) else {
        return false;
    };
    use crate::effects::{Effect, Selector, Trigger};
    adjacent4_w(tile, sim.board_w).any(|a| {
        sim.board[a as usize]
            .spirit
            .as_ref()
            .map(|sp| {
                sp.owner != me.owner
                    && !sp.fading
                    && crate::effects::specs_for(catalog, sp.card).is_some_and(|specs| {
                        specs.iter().any(|s| {
                            s.trigger == Trigger::Static
                                && s.clauses.iter().any(|cl| {
                                    cl.selector == Selector::AdjacentEnemiesAll
                                        && matches!(cl.effect, Effect::TraitSilence)
                                })
                        })
                    })
            })
            .unwrap_or(false)
    })
}

/// The Lullaby: the spirit at `tile` is adjacent to an ENEMY carrying SuppressesAdjacentEnemyEcho
/// — so it is too calm to Echo (its variance bonus is denied).
pub(crate) fn echo_suppressed(sim: &GameState, catalog: &[CardDef], tile: u8) -> bool {
    let Some(me) = sim.spirit_at(tile) else {
        return false;
    };
    adjacent4_w(tile, sim.board_w).any(|a| {
        sim.board[a as usize]
            .spirit
            .as_ref()
            .map(|sp| {
                !sp.fading
                    && sp.owner != me.owner
                    && card_carries_static_exception(
                        catalog,
                        sp.card,
                        crate::effects::RuleException::SuppressesAdjacentEnemyEcho,
                    )
            })
            .unwrap_or(false)
    })
}

/// Hush: a standing spirit adjacent to `tile` whose Static aura carries SuppressesAdjacentParting
/// — so the spirit dissolving here cannot say its goodbye.
pub(crate) fn hush_suppresses_parting(sim: &GameState, catalog: &[CardDef], tile: u8) -> bool {
    adjacent4_w(tile, sim.board_w).any(|a| {
        sim.board[a as usize]
            .spirit
            .as_ref()
            .map(|sp| {
                !sp.fading
                    && card_carries_static_exception(
                        catalog,
                        sp.card,
                        crate::effects::RuleException::SuppressesAdjacentParting,
                    )
            })
            .unwrap_or(false)
    })
}

/// The Long Rest: a standing spirit adjacent to `tile` whose Static aura carries
/// NoImpressionOnAdjacentDissolve — so a spirit dissolving here leaves no impression.
pub(crate) fn adjacent_denies_impression(sim: &GameState, catalog: &[CardDef], tile: u8) -> bool {
    adjacent4_w(tile, sim.board_w).any(|a| {
        sim.board[a as usize]
            .spirit
            .as_ref()
            .map(|sp| {
                !sp.fading
                    && card_carries_static_exception(
                        catalog,
                        sp.card,
                        crate::effects::RuleException::NoImpressionOnAdjacentDissolve,
                    )
            })
            .unwrap_or(false)
    })
}

/// Faultline ("when a spirit fully dissolves here: 10 damage to all adjacent spirits").
/// A Landmark's OnAnyBanish, fired when a spirit dissolves ON its tile — terrain is not a
/// standing witness, so `dissolution_effects` never reaches it. Honors the card text:
/// **all** adjacent spirits (both owners), not just enemies; the magnitude follows the
/// authored `OnAnyBanish/Damage{amount}` clause. Lethal effect-damage dissolves via
/// `banish_or_replace`, laying the Faultline-owner's impression.
pub(crate) fn faultline_dissolve_here(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    tile: u8,
) {
    let Some(terr) = sim.board[tile as usize].terrain.clone() else {
        return;
    };
    if terr.kind != crate::state::TerrainKind::Landmark || terr.face_down {
        return;
    }
    let amount: i16 = crate::effects::specs_for(catalog, terr.card)
        .into_iter()
        .flatten()
        .filter(|s| s.trigger == crate::effects::Trigger::OnAnyBanish)
        .flat_map(|s| &s.clauses)
        .find_map(|cl| match cl.effect {
            crate::effects::Effect::Damage { amount } => Some(amount),
            _ => None,
        })
        .unwrap_or(0);
    if amount == 0 {
        return;
    }
    let targets: Vec<u8> = adjacent4_w(tile, sim.board_w)
        .filter(|&a| sim.spirit_at(a).map(|s| !s.fading).unwrap_or(false))
        .collect();
    for t in targets {
        push(sim, evs, Event::EffectDamaged { tile: t, amount });
        let lethal = sim
            .spirit_at(t)
            .map(|sp| sp.hp <= 0 && !sp.fading)
            .unwrap_or(false);
        if lethal {
            banish_or_replace(sim, evs, catalog, t, terr.owner);
        }
    }
}

pub(crate) fn dissolution_effects(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    _x: Option<CardId>,
) {
    let witnesses: Vec<(u8, CardId, Seat)> = sim
        .board
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.spirit
                .as_ref()
                .filter(|sp| !sp.fading)
                .map(|sp| (i as u8, sp.card, sp.owner))
        })
        .collect();
    for (tile, card_id, owner) in witnesses {
        let name = card(catalog, card_id).name.clone();
        fire_effects_noctx(
            sim,
            evs,
            catalog,
            &name,
            crate::effects::Trigger::OnAnyBanish,
            Some(tile),
            owner,
        );
    }
}

// ---------------------------------------------------------------------------
// Auras are DERIVED state, computed on read — no recompute events, determinism
// by construction. Combat consults combat_stats().

#[derive(Default, Clone, Copy)]
pub(crate) struct CombatStats {
    pub atk: i16,
    pub def: i16,
    pub retaliation: i16,
    pub dmg_reduction: i16,
    pub resonance: Option<Resonance>,
    pub edge_negated_against: bool, // an enemy First Forgotten stands
    pub momentum_first_bonus: bool,
    pub chain_while_defeating: bool,
    pub momentum_per_link_bonus: i16, // Embermane: extra Attack per chain link
    pub chain_no_retaliation: bool,   // Pyrrhic: no retaliation on chain links ≥ 2
}
