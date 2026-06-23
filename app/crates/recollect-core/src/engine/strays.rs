//! Strays: temperament, surfacing, courtship, the Echo-wounded check.
//! A sibling of `clause.rs`; `use super::*` pulls shared helpers.
use super::*;

/// A Foundling's temperament, read from its authored rules prefix.
pub(crate) fn stray_temperament(catalog: &[CardDef], id: CardId) -> crate::state::Temperament {
    use crate::state::Temperament::*;
    let rules = &card(catalog, id).rules;
    if rules.starts_with("Wary") {
        Wary
    } else if rules.starts_with("Feral") {
        Feral
    } else {
        Gentle
    }
}
// ---------------------------------------------------------------------------
// Strays — the Memory remembering. Surfacing is NOT an arrival: no
// engage, no interception, no player zone strikes the newcomer. Inner tiles
// only (the Dusk never reaches them). Telegraph one round ahead names the
// tile, never the identity; occupied-at-surfacing means it does not come.

pub(crate) const INNER_TILES: [u8; 9] = [6, 7, 8, 11, 12, 13, 16, 17, 18];

/// Run at each turn-start: resolve a due surfacing, lay a new telegraph, and
/// advance courtship for an unclaimed Stray.
pub(crate) fn stray_surfacing(sim: &mut GameState, evs: &mut Vec<Event>, ctx: &mut TurnCtx) {
    let round = sim.round;
    // 1) A telegraphed surfacing comes due.
    if let Some(tele) = sim.stray_telegraph.clone()
        && tele.surface_round == round
    {
        // Occupied at surfacing → it does not come (denial = counterplay).
        let tile = tele.tile;
        let open = !sim.board[tile as usize].faded
            && sim.board[tile as usize].spirit.is_none()
            && sim.board[tile as usize].terrain.is_none();
        if open && sim.stray.is_none() {
            // Pick a Foundling (Midnight weights Gentle). Deterministic
            // via entropy over the surfacing pool.
            let pool: Vec<CardId> = ctx
                .catalog
                .iter()
                .filter(|c| {
                    c.kind == CardKind::Foundling
                        && (!tele.midnight || c.rules.starts_with("Gentle"))
                })
                .map(|c| c.id)
                .collect();
            if !pool.is_empty() {
                let pick = pool[ctx.entropy.draw_range(0..pool.len() as u64) as usize];
                let temperament = stray_temperament(&ctx.catalog, pick);
                let veiled = temperament == crate::state::Temperament::Wary;
                let hp = card(&ctx.catalog, pick).hp;
                push(
                    sim,
                    evs,
                    Event::StraySurfaced {
                        card: pick,
                        tile,
                        temperament,
                        veiled,
                        hp,
                    },
                );
            }
        } else {
            // Cancelled: clear the telegraph (pity handled at match level).
            push_clear_telegraph(sim, evs);
        }
    }
    // 2) Courtship for an unclaimed Stray (Gentle: end adjacent w/ shared
    //    Imprint; Wary: two consecutive turns).
    advance_courtship(sim, evs, &ctx.catalog);
}

pub(crate) fn push_clear_telegraph(sim: &mut GameState, evs: &mut Vec<Event>) {
    // Surfacing simply didn't happen — drop the shimmer. This MUST ride an event: the
    // surfacing pass runs inside `decide` on a CLONE, so a bare `stray_telegraph = None`
    // here is lost when only the events are replayed onto the committed board — leaving a
    // stale telegraph (a phantom shimmer) in the view forever. `StrayTelegraphCleared`
    // clears it through `evolve`, on the clone and the real state alike.
    push(sim, evs, Event::StrayTelegraphCleared);
}

/// Feral: a Stray bears an "Echo" once wounded below half its max HP — the
/// pain that finally lets it trust. Only then is a Feral Stray befriendable.
pub(crate) fn stray_is_echo_wounded(stray: &crate::state::Stray, _card: &CardDef) -> bool {
    stray.hp_max > 0 && stray.hp * 2 < stray.hp_max
}

/// A Foundling is befriended: the wild slot empties and the card becomes a normal
/// owned spirit at `tile` (evolve), then its `OnBefriend` doctrine fires through the
/// generic `fire_doctrine` path — same dispatch as every other on-event trigger.
/// Pigeon Carrying a Message Never Delivered draws on this; the message was
/// for whoever befriends it.
fn befriend(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    seat: Seat,
    foundling: CardId,
    tile: u8,
) {
    let d = card(catalog, foundling);
    push(
        sim,
        evs,
        Event::StrayBefriended {
            seat,
            card: foundling,
            tile,
            attack: d.attack,
            defense: d.defense,
            hp: d.hp,
        },
    );
    // The Foundling now stands at `tile` (evolve placed it); its OnBefriend fires
    // with that source/owner, so `Owner`-scoped clauses resolve to the befriender.
    let name = card(catalog, foundling).name.clone();
    fire_doctrine(
        sim,
        evs,
        catalog,
        &name,
        crate::effects::Trigger::OnBefriend,
        Some(tile),
        seat,
    );
}

/// Gentle/Wary courtship. The acting seat that ends adjacent to the Stray
/// courts it; befriending fires when the temperament's threshold is met.
pub(crate) fn advance_courtship(sim: &mut GameState, evs: &mut Vec<Event>, catalog: &[CardDef]) {
    let Some(stray) = sim.stray.clone() else {
        return;
    };
    let seat = sim.active;
    let adjacent_ally = sim.board.iter().enumerate().any(|(i, t)| {
        t.spirit
            .as_ref()
            .map(|sp| !sp.fading && sp.owner == seat && manhattan(stray.tile, i as u8) == 1)
            .unwrap_or(false)
    });
    // A VEILED (Wary) Stray unveils when a teller ends adjacent — and that
    // adjacency counts as the first courtship turn. With no adjacency for two
    // rounds it self-unveils (curiosity wins); we model the patience unveil as
    // unveiling on the second turn it has stood unseen.
    if stray.veiled {
        if adjacent_ally {
            push(sim, evs, Event::StrayUnveiled);
            push(sim, evs, Event::StrayCourted { seat, courtship: 1 });
        } else {
            // Patience: track near-sightings via courtship counter while veiled.
            let seen = stray.courtship + 1;
            if seen >= 2 {
                push(sim, evs, Event::StrayUnveiled);
            } else {
                push(
                    sim,
                    evs,
                    Event::StrayCourted {
                        seat,
                        courtship: seen,
                    },
                );
            }
        }
        return;
    }
    if !adjacent_ally {
        return;
    }
    use crate::state::Temperament::*;
    match stray.temperament {
        Gentle => {
            // One adjacency with a shared Imprint befriends.
            let stray_imprints = &card(catalog, stray.card).imprints;
            let shares = sim.board.iter().enumerate().any(|(i, t)| {
                t.spirit
                    .as_ref()
                    .map(|sp| {
                        !sp.fading
                            && sp.owner == seat
                            && manhattan(stray.tile, i as u8) == 1
                            && card(catalog, sp.card)
                                .imprints
                                .iter()
                                .any(|im| stray_imprints.contains(im))
                    })
                    .unwrap_or(false)
            });
            if shares {
                befriend(sim, evs, catalog, seat, stray.card, stray.tile);
            }
        }
        Wary => {
            // Two consecutive turns by the same seat (post-unveil).
            let next = if stray.courted_by == Some(seat) {
                stray.courtship + 1
            } else {
                1
            };
            if next >= 2 {
                befriend(sim, evs, catalog, seat, stray.card, stray.tile);
            } else {
                push(
                    sim,
                    evs,
                    Event::StrayCourted {
                        seat,
                        courtship: next,
                    },
                );
            }
        }
        Feral => {
            // A Feral Stray is befriendable ONLY while bearing an Echo
            // (below half its max HP — wounded into trust). Its interception of
            // arrivals is handled in the interception path; here, an adjacent
            // ally with a shared Imprint befriends it IF it is Echo-wounded.
            let d = card(catalog, stray.card).clone();
            let echo_wounded = stray_is_echo_wounded(&stray, &d);
            if echo_wounded {
                let stray_imprints = &d.imprints;
                let shares = sim.board.iter().enumerate().any(|(i, t)| {
                    t.spirit
                        .as_ref()
                        .map(|sp| {
                            !sp.fading
                                && sp.owner == seat
                                && manhattan(stray.tile, i as u8) == 1
                                && card(catalog, sp.card)
                                    .imprints
                                    .iter()
                                    .any(|im| stray_imprints.contains(im))
                        })
                        .unwrap_or(false)
                });
                if shares {
                    befriend(sim, evs, catalog, seat, stray.card, stray.tile);
                }
            }
        }
    }
}
