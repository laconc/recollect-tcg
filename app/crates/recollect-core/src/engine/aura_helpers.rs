//! Static aura / buff helpers: bond-pair shares, terrain/landmark deltas, cost & push-immunity auras, static-exception carriers.
//! A sibling of `clause.rs`; `use super::*` pulls shared helpers.
use super::*;

/// Magistrate / Warden-Breaker: push the self-buff a spirit carries as `trigger`/SelfSpirit/
/// StatDelta (a permanent EffectStat). No-op if the spirit lacks it or has faded.
pub(crate) fn fire_self_stat_buff(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    tile: u8,
    trigger: crate::effects::Trigger,
) {
    let card_id = match sim.spirit_at(tile) {
        Some(sp) if !sp.fading => sp.card,
        _ => return,
    };
    let Some(specs) = crate::effects::specs_for(catalog, card_id) else {
        return;
    };
    let buffs: Vec<(i16, i16, i16)> = specs
        .iter()
        .filter(|s| s.trigger == trigger)
        .flat_map(|s| &s.clauses)
        .filter_map(|cl| match (&cl.selector, &cl.effect) {
            (
                crate::effects::Selector::SelfSpirit,
                crate::effects::Effect::StatDelta {
                    attack,
                    defense,
                    form,
                },
            ) => Some((*attack, *defense, *form)),
            _ => None,
        })
        .collect();
    for (attack, defense, form) in buffs {
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

/// Magistrate of Masks: any Fabrication revealing anywhere buffs every standing Magistrate.
pub(crate) fn buff_on_fab_reveal(sim: &mut GameState, evs: &mut Vec<Event>, catalog: &[CardDef]) {
    let tiles: Vec<u8> = (0..sim.board.len() as u8)
        .filter(|&i| {
            sim.spirit_at(i)
                .map(|sp| {
                    !sp.fading
                        && crate::effects::specs_for(catalog, sp.card)
                            .map(|specs| {
                                specs.iter().any(|s| {
                                    s.trigger == crate::effects::Trigger::OnFabricationRevealed
                                })
                            })
                            .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .collect();
    for t in tiles {
        fire_self_stat_buff(
            sim,
            evs,
            catalog,
            t,
            crate::effects::Trigger::OnFabricationRevealed,
        );
    }
}

/// Elegist Wren: an allied spirit Parting at `parting_tile` grows each of `owner`'s Wrens
/// whose Reach covers it (OnAllyPartsInReach/SelfSpirit/StatDelta).
pub(crate) fn buff_elegist_wrens(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    owner: Seat,
    parting_tile: u8,
) {
    let wrens: Vec<u8> = (0..sim.board.len() as u8)
        .filter(|&i| {
            if i == parting_tile {
                return false;
            }
            let Some(sp) = sim.spirit_at(i) else {
                return false;
            };
            if sp.owner != owner || sp.fading {
                return false;
            }
            let has = crate::effects::specs_for(catalog, sp.card)
                .map(|specs| {
                    specs
                        .iter()
                        .any(|s| s.trigger == crate::effects::Trigger::OnAllyPartsInReach)
                })
                .unwrap_or(false);
            has && targeting_reach(
                sim,
                catalog,
                card(catalog, sp.card).reach,
                i,
                owner,
                sim.board_w,
            )
            .contains(&parting_tile)
        })
        .collect();
    for w in wrens {
        fire_self_stat_buff(
            sim,
            evs,
            catalog,
            w,
            crate::effects::Trigger::OnAllyPartsInReach,
        );
    }
}

/// Arbiter Imperishable: does `seat` control a standing spirit whose Static aura makes its
/// allies unpushable (AlliesAll/Restrict(BePushed))?
pub(crate) fn seat_grants_push_immunity(sim: &GameState, catalog: &[CardDef], seat: Seat) -> bool {
    sim.board.iter().any(|t| {
        t.spirit
            .as_ref()
            .map(|sp| {
                sp.owner == seat
                    && !sp.fading
                    && !sp.face_down
                    && crate::effects::specs_for(catalog, sp.card)
                        .map(|specs| {
                            specs.iter().any(|s| {
                                s.trigger == crate::effects::Trigger::Static
                                    && s.clauses.iter().any(|cl| {
                                        cl.selector == crate::effects::Selector::AlliesAll
                                            && matches!(&cl.effect, crate::effects::Effect::Restrict(r) if *r == crate::effects::Restriction::BePushed)
                                    })
                            })
                        })
                        .unwrap_or(false)
            })
            .unwrap_or(false)
    })
}

/// Rondel, the Joining: does `seat` control a STANDING spirit whose Static spec carries
/// BondedPair/StatShareHigher? Returns the (attack, defense) share flags it grants to
/// every one of the seat's bonded pairs (a seat-wide aura, unlike a bond card's).
pub(crate) fn owner_grants_pair_share(
    sim: &GameState,
    catalog: &[CardDef],
    seat: Seat,
) -> (bool, bool) {
    let mut share_atk = false;
    let mut share_def = false;
    for t in &sim.board {
        let Some(sp) = &t.spirit else { continue };
        if sp.owner != seat || sp.fading || sp.face_down {
            continue;
        }
        let Some(specs) = crate::effects::specs_for(catalog, sp.card) else {
            continue;
        };
        for spec in specs {
            if spec.trigger != crate::effects::Trigger::Static {
                continue;
            }
            for cl in &spec.clauses {
                if cl.selector == crate::effects::Selector::BondedPair
                    && let crate::effects::Effect::StatShareHigher { attack, defense } = cl.effect
                {
                    share_atk |= attack;
                    share_def |= defense;
                }
            }
        }
    }
    (share_atk, share_def)
}
/// Duet Ascendant, Both Halves: the FLAT bond stat `seat` grants its bonded spirits (summed
/// over its standing Duets, via Static Owner/BondStatGrant).
pub(crate) fn owner_grants_bond_stat(
    sim: &GameState,
    catalog: &[CardDef],
    seat: Seat,
) -> (i16, i16) {
    let mut a = 0;
    let mut d = 0;
    for t in &sim.board {
        let Some(sp) = &t.spirit else { continue };
        if sp.owner != seat || sp.fading || sp.face_down {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, sp.card) {
            for spec in specs
                .iter()
                .filter(|s| s.trigger == crate::effects::Trigger::Static)
            {
                for cl in &spec.clauses {
                    if cl.selector == crate::effects::Selector::Owner
                        && let crate::effects::Effect::BondStatGrant { attack, defense } = cl.effect
                    {
                        a += attack;
                        d += defense;
                    }
                }
            }
        }
    }
    (a, d)
}
/// Sum of CostDelta auras a seat controls (Star-Strewn Otter, Lurking
/// Court). Negative = cheaper. Applied to placement legality and spend.
/// Ink Runs Dry: if `seat` carries an active card-tax surcharge, spend it now (a play just
/// paid it). A no-op when no tax is active, so it is safe to call at every play site.
pub(crate) fn spend_card_tax(sim: &mut GameState, evs: &mut Vec<Event>, seat: Seat) {
    let (tax, until) = sim.card_tax[seat as usize];
    if tax > 0 && until >= sim.round {
        push(sim, evs, Event::CardTaxSpent { seat });
    }
}

pub(crate) fn cost_aura(st: &GameState, seat: Seat, catalog: &[CardDef]) -> i16 {
    // Ink Runs Dry: a this-round surcharge on this seat's next card (spirit or Ritual).
    let (tax, until) = st.card_tax[seat as usize];
    let mut delta = if until >= st.round { tax as i16 } else { 0 };
    for t in &st.board {
        if let Some(sp) = &t.spirit {
            if sp.fading || sp.owner != seat {
                continue;
            }
            if let Some(specs) = crate::effects::specs_for(catalog, sp.card) {
                for s in specs {
                    if s.trigger != crate::effects::Trigger::Static {
                        continue;
                    }
                    for cl in &s.clauses {
                        if let crate::effects::Effect::CostDelta { delta: d } = cl.effect {
                            delta += d as i16;
                        }
                    }
                }
            }
        }
    }
    delta
}
/// The Long Shadow: a face-up Landmark adjacent to `tile`, owned by `seat`, that carries
/// Owner/CostDelta reduces the cost of a Fabrication placed at `tile`. cost_aura reads
/// only spirit auras, so terrain CostDelta is realized here, positionally. Returns the
/// (negative) cost delta to add.
pub(crate) fn adjacent_landmark_fab_delta(
    st: &GameState,
    catalog: &[CardDef],
    seat: Seat,
    tile: u8,
) -> i16 {
    let mut delta = 0i16;
    for i in 0..st.board.len() as u8 {
        if manhattan(tile, i) != 1 {
            continue;
        }
        let Some(terr) = &st.board[i as usize].terrain else {
            continue;
        };
        if terr.face_down || terr.owner != seat {
            continue;
        }
        if let Some(specs) = crate::effects::specs_for(catalog, terr.card) {
            for s in specs
                .iter()
                .filter(|s| s.trigger == crate::effects::Trigger::Static)
            {
                for cl in &s.clauses {
                    if cl.selector == crate::effects::Selector::Owner
                        && let crate::effects::Effect::CostDelta { delta: d } = cl.effect
                    {
                        delta += d as i16;
                    }
                }
            }
        }
    }
    delta
}
/// Beacon: does `seat` control a face-up Landmark carrying Static RevealFabrication
/// adjacent to `tile`? Such a landmark reveals adjacent enemy Fabrications to its owner
/// (consulted by the view's redaction — no state change, no leak to the opponent).
pub(crate) fn beacon_reveals_fab(
    st: &GameState,
    catalog: &[CardDef],
    seat: Seat,
    tile: u8,
) -> bool {
    (0..st.board.len() as u8).any(|i| {
        manhattan(tile, i) == 1
            && st.board[i as usize]
                .terrain
                .as_ref()
                .map(|tr| {
                    !tr.face_down
                        && tr.owner == seat
                        && tr.kind == crate::state::TerrainKind::Landmark
                        && crate::effects::specs_for(catalog, tr.card)
                            .map(|specs| {
                                specs.iter().any(|s| {
                                    s.trigger == crate::effects::Trigger::Static
                                        && s.clauses.iter().any(|cl| {
                                            matches!(
                                                cl.effect,
                                                crate::effects::Effect::RevealFabrication
                                            )
                                        })
                                })
                            })
                            .unwrap_or(false)
                })
                .unwrap_or(false)
    })
}
/// Shrine of the Nameless: the owner controls a face-up Landmark carrying
/// OccupantHere/Exception(FadingFuelIgnoresImprint) whose tile holds a FADING owned spirit
/// — that lingering memory fuels the owner's evolutions imprint-free.
pub(crate) fn shrine_fading_fuel(st: &GameState, catalog: &[CardDef], owner: Seat) -> bool {
    (0..st.board.len() as u8).any(|i| {
        let t = &st.board[i as usize];
        t.terrain
            .as_ref()
            .map(|tr| {
                !tr.face_down
                    && tr.owner == owner
                    && card_carries_static_exception(
                        catalog,
                        tr.card,
                        crate::effects::RuleException::FadingFuelIgnoresImprint,
                    )
            })
            .unwrap_or(false)
            && t.spirit
                .as_ref()
                .map(|sp| sp.owner == owner && sp.fading)
                .unwrap_or(false)
    })
}

/// What's-Its-Name: a spirit whose Static aura carries `Restrict(BeTargetedByRituals)`
/// cannot be selected as a Ritual's (or any free-target effect's) target — the unnameable
/// thing slips the caster's aim. A card-level read (the restriction is SelfSpirit), consulted
/// at the target-eligibility chokepoint (`exec_target_spirit`).
pub(crate) fn ritual_untargetable(catalog: &[CardDef], card_id: CardId) -> bool {
    use crate::effects::{Effect, Restriction, Selector, Trigger};
    crate::effects::specs_for(catalog, card_id).is_some_and(|specs| {
        specs.iter().any(|s| {
            s.trigger == Trigger::Static
                && s.clauses.iter().any(|cl| {
                    cl.selector == Selector::SelfSpirit
                        && matches!(
                            cl.effect,
                            Effect::Restrict(Restriction::BeTargetedByRituals)
                        )
                })
        })
    })
}

// ── Throughline detection ───────────────────────────────────────────────
/// A spirit's card carries a Static Exception(`which`) (Errata's AllImprintsShared, an
/// Unbreakable/Twin-Telling bond's exception).
pub(crate) fn card_carries_static_exception(
    catalog: &[CardDef],
    card_id: CardId,
    which: crate::effects::RuleException,
) -> bool {
    // O(1) id → spec seam (no name clone / name→key hash), with the same predicate as
    // `name_carries_static_exception` — a Static spec with any Exception(`which`) clause.
    crate::effects::specs_for(catalog, card_id)
        .map(|specs| {
            specs.iter().any(|s| {
                s.trigger == crate::effects::Trigger::Static
                    && s.clauses.iter().any(
                        |cl| matches!(&cl.effect, crate::effects::Effect::Exception(x) if *x == which),
                    )
            })
        })
        .unwrap_or(false)
}

/// Whether the card with this name has a Static/SelfSpirit Exception(`which`) clause — the
/// catalog-free path the forecast uses (it holds a `&CardDef`, not the catalog).
pub(crate) fn name_carries_static_exception(
    name: &str,
    which: crate::effects::RuleException,
) -> bool {
    crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(name))
        .map(|specs| {
            specs.iter().any(|s| {
                s.trigger == crate::effects::Trigger::Static
                    && s.clauses.iter().any(
                        |cl| matches!(&cl.effect, crate::effects::Effect::Exception(x) if *x == which),
                    )
            })
        })
        .unwrap_or(false)
}
