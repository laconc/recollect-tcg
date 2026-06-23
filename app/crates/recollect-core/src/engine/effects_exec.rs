//! Effect execution: target selection, the clause interpreter, fire_* dispatch.
//! A sibling of `engine.rs`; `use super::*` pulls shared helpers + crate types.
use super::*;

/// Resolve an effect's `Selector` to the concrete tiles it hits (self, allies,
/// adjacent enemies, the engager, in-reach, etc.). The single place selector
/// semantics live, so every effect targets consistently.
pub(crate) fn effect_targets(
    sim: &GameState,
    sel: &crate::effects::Selector,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) -> Vec<u8> {
    use crate::effects::Selector as S;
    let standing = |pred: &dyn Fn(u8, &Spirit) -> bool| -> Vec<u8> {
        sim.board
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.spirit.as_ref().map(|sp| (i as u8, sp)))
            .filter(|(i, sp)| !sp.fading && pred(*i, sp))
            .map(|(i, _)| i)
            .collect()
    };
    match sel {
        S::SelfSpirit => source.into_iter().collect(),
        S::Engager => engager.into_iter().collect(),
        // The enemy the source engaged. In the instant (on-arrival) path it is
        // carried as `engager` when present; otherwise fall back to the nearest
        // adjacent enemy (the one a shove would target).
        S::EngagedEnemy => {
            if let Some(en) = engager {
                return vec![en];
            }
            let Some(src) = source else { return vec![] };
            standing(&|i, sp| {
                let (sx, sy) = tile_xy(src);
                let (ix, iy) = tile_xy(i);
                sp.owner != owner
                    && (sx as i16 - ix as i16).abs() + (sy as i16 - iy as i16).abs() == 1
            })
            .into_iter()
            .take(1)
            .collect()
        }
        // Owner picks one adjacent enemy; resolved here as the first adjacent
        // enemy (the instant/doctrine path doesn't open a choice for these).
        S::AdjacentEnemyChoose => {
            let Some(src) = source else { return vec![] };
            standing(&|i, sp| {
                let (sx, sy) = tile_xy(src);
                let (ix, iy) = tile_xy(i);
                sp.owner != owner
                    && (sx as i16 - ix as i16).abs() + (sy as i16 - iy as i16).abs() == 1
            })
            .into_iter()
            .take(1)
            .collect()
        }
        S::AlliesAll => standing(&|_, sp| sp.owner == owner),
        S::EnemiesAll => standing(&|_, sp| sp.owner != owner),
        S::AllOtherSpirits => standing(&|i, _| Some(i) != source),
        S::AdjacentEnemiesAll => {
            let Some(src) = source else { return vec![] };
            standing(&|i, sp| {
                let (sx, sy) = tile_xy(src);
                let (ix, iy) = tile_xy(i);
                sp.owner != owner
                    && (sx as i16 - ix as i16).abs() + (sy as i16 - iy as i16).abs() == 1
            })
        }
        S::AdjacentAlliesAll => {
            // Allied spirits orthogonally adjacent to the source. Used by instant
            // heals/buffs (e.g. RestoreForm on a fading neighbour). Mirrors the
            // derived-combat path's AdjacentAlliesAll so both agree.
            let Some(src) = source else { return vec![] };
            standing(&|i, sp| {
                let (sx, sy) = tile_xy(src);
                let (ix, iy) = tile_xy(i);
                sp.owner == owner
                    && Some(i) != source
                    && (sx as i16 - ix as i16).abs() + (sy as i16 - iy as i16).abs() == 1
            })
        }
        _ => vec![],
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ChoiceMode {
    /// Own-turn trigger: choice clauses open a PendingChoice.
    Open,
    /// Parting: choice clauses resolve by doctrine — the dying don't deliberate.
    Doctrine,
}

/// Does this card carry an `ImpressionEat` clause (The Devouring Margin)? The shift
/// computes it from the card's spec.
fn card_eats_impression(catalog: &[CardDef], id: CardId) -> bool {
    crate::effects::specs_for(catalog, id).is_some_and(|specs| {
        specs.iter().any(|s| {
            s.clauses
                .iter()
                .any(|cl| matches!(cl.effect, crate::effects::Effect::ImpressionEat))
        })
    })
}

/// The Devouring Margin: how much it heals when it eats on the inward shift —
/// the `RestoreForm` rider on its OnMove spec ("heals 10 when it does"). The shift
/// path realizes the OnMove eat directly (there is no player-driven move for a Solace
/// eater), so its paired heal is read here from the same spec. `0` if none.
fn card_move_heal(catalog: &[CardDef], id: CardId) -> i16 {
    crate::effects::specs_for(catalog, id)
        .into_iter()
        .flatten()
        .filter(|s| s.trigger == crate::effects::Trigger::OnMove)
        .flat_map(|s| &s.clauses)
        .find_map(|cl| match cl.effect {
            crate::effects::Effect::RestoreForm { amount } => Some(amount),
            _ => None,
        })
        .unwrap_or(0)
}

pub(crate) fn exec_clause_mode(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
    mode: ChoiceMode,
    // The Fog of Elsewhere: a CostAtMost spec condition caps which targets are
    // eligible (here, enemies of Cost ≤ cap) rather than gating the whole clause.
    cost_cap: Option<u8>,
    // Ragewoken Bison: a PayForm spec condition's HP cost, offered at the engage choice.
    pay_hp: Option<i16>,
) {
    use crate::effects::{Duration, Effect as E, Selector as S};
    // ImpressionEat — a Solace on-move eater (The Devouring Margin) lands on the player's
    // impression and consumes it: the mark is gone and the Solace's erasure tally goes +1 (deny +
    // score, the same swing as a banish). It only eats seat A's mark, never the Solace's own.
    if matches!(cl.effect, E::ImpressionEat) {
        if let Some(tile) = source
            && owner == Seat::B
            && sim.board[tile as usize].impressions.contains(&Seat::A)
        {
            push(sim, evs, Event::ImpressionForgotten { tile });
        }
        return;
    }
    // The Page Turns: every standing Unwritten steps one tile toward the Memory's center
    // (when the inward tile is open). Farthest-first so inner movers clear the way for outer ones.
    if matches!(cl.effect, E::ShiftUnwrittenInward) {
        let w = sim.board_w;
        let c = (w - 1) / 2;
        let dist = |t: u8| -> i32 {
            let (x, y) = crate::types::tile_xy_w(t, w);
            ((x - c).abs() + (y - c).abs()) as i32
        };
        let mut tiles: Vec<u8> = (0..sim.board.len() as u8)
            .filter(|&t| {
                sim.board[t as usize].spirit.as_ref().is_some_and(|sp| {
                    !sp.fading
                        && catalog
                            .iter()
                            .find(|c| c.id == sp.card)
                            .is_some_and(|c| c.kind.is_antagonist_creature())
                })
            })
            .collect();
        tiles.sort_by_key(|&t| std::cmp::Reverse(dist(t)));
        let mut claimed: Vec<u8> = Vec::new();
        for from in tiles {
            let (x, y) = crate::types::tile_xy_w(from, w);
            let (sx, sy) = ((c - x).signum(), (c - y).signum());
            let to = if (x - c).abs() >= (y - c).abs() && sx != 0 {
                crate::types::xy_tile_w(x + sx, y, w)
            } else if sy != 0 {
                crate::types::xy_tile_w(x, y + sy, w)
            } else if sx != 0 {
                crate::types::xy_tile_w(x + sx, y, w)
            } else {
                None
            };
            if let Some(to) = to
                // An open landing only — no spirit, terrain (inv #1), or Stray (1b); the
                // wild blocks an inward shift just as a standing spirit does.
                && tile_open_for_arrival(sim, to)
                && !claimed.contains(&to)
            {
                // A mover carrying `ImpressionEat` (The Devouring Margin) eats the player's mark it
                // lands on — that is what makes forgetting score (the erasure is tallied). The shift
                // computes this directly; non-eaters leave the mark be. (Capture the mover's card
                // BEFORE the shift — `push` applies UnwrittenShifted eagerly, emptying `from`.)
                let mover_card = sim.board[from as usize].spirit.as_ref().map(|sp| sp.card);
                let eats_impression = mover_card.is_some_and(|c| card_eats_impression(catalog, c))
                    && sim.board[to as usize].impressions.contains(&Seat::A);
                push(
                    sim,
                    evs,
                    Event::UnwrittenShifted {
                        from,
                        to,
                        eats_impression,
                    },
                );
                // "heals 10 when it does": the eater restores form on the eat (read from its
                // OnMove RestoreForm rider). Pushed AFTER the shift, so the spirit is at `to`.
                if eats_impression && let Some(c) = mover_card {
                    let heal = card_move_heal(catalog, c);
                    if heal > 0 {
                        push(
                            sim,
                            evs,
                            Event::EffectRestored {
                                tile: to,
                                amount: heal,
                            },
                        );
                    }
                }
                claimed.push(to);
            }
        }
        return;
    }
    // A Mercy for the Rim: release every held spirit on a rim tile (gently — no impression).
    if matches!(cl.effect, E::ReleaseHeldRim) {
        let w = sim.board_w;
        for t in 0..sim.board.len() as u8 {
            if crate::types::is_rim_w(t, w)
                && sim.board[t as usize]
                    .spirit
                    .as_ref()
                    .is_some_and(|sp| sp.holding && !sp.fading)
            {
                push(sim, evs, Event::SpiritReleased { tile: t });
            }
        }
        return;
    }
    // Silence Spreads: the first standing (face-up) Landmark loses its text this round.
    if matches!(cl.effect, E::SilenceLandmark) {
        for t in 0..sim.board.len() as u8 {
            if sim.board[t as usize].terrain.as_ref().is_some_and(|tr| {
                matches!(tr.kind, crate::state::TerrainKind::Landmark) && !tr.face_down
            }) {
                push(
                    sim,
                    evs,
                    Event::LandmarkSilenced {
                        tile: t,
                        round: sim.round,
                    },
                );
                break;
            }
        }
        return;
    }
    // Lost Paragraph: manifest a cost-2 Unwritten on a free rim tile.
    if matches!(cl.effect, E::ManifestUnwrittenOnRim) {
        let w = sim.board_w;
        let tile = (0..sim.board.len() as u8).find(|&t| {
            crate::types::is_rim_w(t, w)
                && sim.board[t as usize].spirit.is_none()
                && sim.board[t as usize].terrain.is_none() // a body never lands on terrain (inv #1)
                && !sim.board[t as usize].faded
        });
        let card = catalog
            .iter()
            .find(|c| c.cost == 2 && c.kind.is_antagonist_creature());
        if let (Some(tile), Some(c)) = (tile, card) {
            push(
                sim,
                evs,
                Event::UnwrittenManifested {
                    card: c.id,
                    tile,
                    attack: c.attack,
                    defense: c.defense,
                    hp: c.hp,
                },
            );
        }
        return;
    }
    // Let It Lie: impressions stop scoring for a round (go dormant).
    if matches!(cl.effect, E::StopImpressionScoring) {
        push(
            sim,
            evs,
            Event::ImpressionsStoppedScoring { round: sim.round },
        );
        return;
    }
    // The Quiet Spreads: two inner tiles go calm (uncallable) next round.
    if matches!(cl.effect, E::ScheduleCalmTiles) {
        let w = sim.board_w;
        let inner: Vec<u8> = (0..sim.board.len() as u8)
            .filter(|&t| !crate::types::is_rim_w(t, w))
            .take(2)
            .collect();
        if !inner.is_empty() {
            push(
                sim,
                evs,
                Event::TilesGoneCalm {
                    tiles: inner,
                    round: sim.round + 1,
                },
            );
        }
        return;
    }
    // Choice-bearing selectors. Release/Banish resolve directly (Release's dying
    // don't deliberate, and a choice UI that hides fading spirits couldn't offer
    // them; Banish's adjacent-enemy erasure is doctrine-resolved like Release), so
    // they skip this routing and fall through to the direct executor below.
    if matches!(cl.selector, S::AdjacentAllyChoose | S::AdjacentEnemyChoose)
        && !matches!(cl.effect, E::Release | E::Banish)
    {
        exec_adjacent_ally_choose(sim, evs, catalog, cl, source, owner, mode);
        return;
    }
    if matches!(cl.effect, E::RevealFabrication) && cl.selector == S::Owner {
        exec_reveal_fabrication(sim, evs, owner, mode);
        return;
    }
    if matches!(cl.effect, E::RevealFabrication)
        && matches!(
            cl.selector,
            S::TargetSpirit | S::TargetAllySpirit | S::TargetEnemySpirit
        )
    {
        exec_reveal_fabrication_2(sim, evs, owner, mode);
        return;
    }
    if cl.selector == S::TargetFadingSpirit
        && matches!(
            cl.effect,
            E::Replace(crate::effects::Replacement::DelayFadeOneStep)
        )
    {
        exec_target_fading_spirit(sim, evs, owner, mode);
        return;
    }
    if matches!(
        cl.selector,
        S::TargetSpirit | S::TargetAllySpirit | S::TargetEnemySpirit
    ) {
        exec_target_spirit(sim, evs, catalog, cl, source, owner, mode, cost_cap);
        return;
    }
    if cl.selector == S::TargetTwoAdjacentAllies {
        exec_target_two_adjacent_allies(sim, evs, cl, owner, mode);
        return;
    }
    if cl.selector == S::TargetBondedPair {
        exec_target_bonded_pair(sim, evs, cl, owner, mode);
        return;
    }
    if let E::PeekDeck { look, take } = &cl.effect {
        // A Peek is an INTERACTIVE choice — it can only be opened for the player
        // whose turn it is. If a PeekDeck effect triggers for a non-active owner
        // (e.g. on a Parting or OnAnyBanish during the opponent's turn), opening
        // it would leave a dangling cross-turn choice and soft-lock the match
        // (the async law forbids opponent-turn windows). In that case we skip it
        // rather than strand the player — a peek with no one to act on it does
        // nothing, consistent with doctrine resolution for non-active effects.
        if mode == ChoiceMode::Open && *take > 0 && cl.selector == S::Owner && sim.active == owner {
            // Zenith's "Your Glimpses take +1 card": the owner controlling a
            // GlimpseLooksOneMore carrier looks at one more before taking.
            let bonus = exception_active(
                sim,
                catalog,
                owner,
                crate::effects::RuleException::GlimpseLooksOneMore,
            ) as usize;
            let p = sim.player(owner);
            let n = (*look as usize + bonus).min(p.deck.len());
            if n == 0 {
                return;
            }
            let looked: Vec<CardId> = p.deck[p.deck.len() - n..].to_vec();
            // (deck top is the END in this engine, matching CardDrawn.)
            let sim2 = sim;
            push(
                sim2,
                evs,
                Event::ChoiceOffered {
                    choice: crate::state::PendingChoice::Peek {
                        seat: owner,
                        looked,
                    },
                },
            );
        }
        return;
    }
    // Push: deterministic shove away from the source. AdjacentEnemiesAll hits
    // every adjacent enemy (Empty Procession); EngagedEnemy/AdjacentEnemyChoose
    // shove the single targeted enemy (The Vanishing Point, ill-intent shoves).
    if let E::Displace(crate::effects::Displacement::Push { tiles }) = &cl.effect {
        if matches!(
            cl.selector,
            S::AdjacentEnemiesAll | S::EngagedEnemy | S::AdjacentEnemyChoose | S::Engager
        ) {
            let Some(src) = source else { return };
            let targets = effect_targets(sim, &cl.selector, source, owner, engager);
            for t in targets {
                push_away(sim, evs, catalog, src, t, *tiles, owner);
            }
        }
        return;
    }
    // Bounce: return the target to its owner's hand (The Unasked Question shoos the
    // engager that sprang it). Resolves via effect_targets, like the shoves above.
    if matches!(cl.effect, E::Bounce) {
        exec_bounce(sim, evs, cl, source, owner, engager);
        return;
    }
    if let E::MillTopDeck { opponent } = cl.effect {
        let seat = if opponent { owner.other() } else { owner };
        push(sim, evs, Event::DeckMilled { seat });
        return;
    }
    // The Half-Remembered: on reveal, copy the owner's last face-up spirit's stats.
    if matches!(cl.effect, E::CopyLastPlayed) {
        exec_copy_last_played(sim, evs, catalog, source, owner);
        return;
    }
    if matches!(cl.effect, E::CopyEngagerReach) {
        exec_copy_engager_reach(sim, evs, catalog, source, engager);
        return;
    }
    // The Misremembered: on engage-resolved, copy the printed Attack/Defense of the spirit
    // it just fought (the engager bound in the OnEngageResolved fire). HP is untouched —
    // "copies the printed A/D" — so it keeps its own form.
    if matches!(cl.effect, E::CopyPrintedStats) {
        exec_copy_printed_stats(sim, evs, catalog, source, engager);
        return;
    }
    if matches!(cl.effect, E::TakeControl) {
        exec_take_control(sim, evs, cl, source, owner, engager);
        return;
    }
    if matches!(cl.effect, E::ImpressionRemoveTarget) {
        exec_impression_remove_target(sim, evs, cl, owner, mode);
        return;
    }
    if matches!(cl.effect, E::TraitStrip) {
        exec_trait_strip(sim, evs, cl, source, owner, engager);
        return;
    }
    if matches!(cl.effect, E::Recover) {
        exec_recover(sim, evs, cl, owner, mode);
        return;
    }
    if let E::ReachDelta {
        forward,
        all_directions,
        targeting_only,
    } = &cl.effect
    {
        // Seat-wide reach buff (Tempestrider Roc &c. targeting-only; Open Sky full).
        if cl.selector == S::AlliesAll && cl.duration == Duration::ThisRound {
            push(
                sim,
                evs,
                Event::ReachBuffed {
                    seat: owner,
                    forward: *forward,
                    all_directions: *all_directions,
                    until_round: sim.round,
                    targeting_only: *targeting_only,
                    tile: None,
                },
            );
        }
        return;
    }
    // This-round seat-wide movement/push restriction (Stand Ground): recorded as a
    // `Restricted` and honored by `restricted()` at the move + push call sites.
    if let E::Restrict(restriction) = &cl.effect {
        if cl.selector == S::AlliesAll && cl.duration == Duration::ThisRound {
            push(
                sim,
                evs,
                Event::Restricted {
                    seat: owner,
                    restriction: *restriction,
                    until_round: sim.round,
                },
            );
        }
        return;
    }
    // Ragewoken Bison: SelfSpirit/ExtraEngage gated by PayForm — offer a target choice
    // (enemies in the source's reach) plus the source itself as a decline (the "may pay").
    if cl.selector == S::SelfSpirit && matches!(cl.effect, E::ExtraEngage) {
        exec_self_spirit(sim, evs, catalog, source, owner, mode, pay_hp);
        return;
    }
    if matches!(cl.effect, E::ReTriggerParting) {
        exec_re_trigger_parting(sim, evs, catalog, source, owner, mode);
        return;
    }
    if cl.selector == S::NextArrivalThisTurn {
        exec_next_arrival_this_turn(sim, evs, cl, owner);
        return;
    }
    if matches!(
        cl.effect,
        E::Exception(crate::effects::RuleException::EvolveIgnoresSharedImprint)
    ) {
        exec_exception(sim, evs, cl, owner);
        return;
    }
    if let E::Summon { card_name } = &cl.effect {
        summon_kindred(sim, evs, catalog, card_name, source, owner);
        return;
    }
    apply_direct_clause(sim, evs, catalog, cl, source, owner, engager);
}

fn apply_direct_clause(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    catalog: &[CardDef],
    cl: &crate::effects::Clause,
    source: Option<u8>,
    owner: Seat,
    engager: Option<u8>,
) {
    use crate::effects::{Duration, Effect as E, Selector as S};
    if !crate::effects::supported_instant_clause(cl) {
        return;
    }
    match &cl.effect {
        E::Release => {
            // The Solace's mercy: release the targeted FADING spirits, leaving no
            // impression (the dying only — release_targets is fading-only). Any-owner
            // and fading-inclusive, so we select directly rather than via
            // effect_targets (owner-scoped, skips fading).
            for t in release_targets(sim, &cl.selector, source, owner) {
                push(sim, evs, Event::SpiritReleased { tile: t });
            }
        }
        E::Banish => {
            // The IllIntent erasure: banish the targeted spirits outright — healthy
            // included — leaving no impression (banish_targets does not filter on
            // fading). Same SpiritReleased event (remove, no impression); the
            // distinction from Release is purely which targets are eligible.
            for t in banish_targets(sim, &cl.selector, source, owner) {
                push(sim, evs, Event::SpiritReleased { tile: t });
            }
        }

        E::Draw { count } => {
            // Owner draws; BothNarrators draws for each side (Trade Winds).
            let seats: &[Seat] = match cl.selector {
                S::Owner => &[owner],
                S::BothNarrators => &[Seat::A, Seat::B],
                _ => &[],
            };
            for &seat in seats {
                for _ in 0..*count {
                    if !sim.player(seat).deck.is_empty() {
                        push(sim, evs, Event::CardDrawn { seat });
                    }
                }
            }
        }
        E::AnimaDelta { amount } if *amount > 0 && cl.selector == S::Owner => {
            push(
                sim,
                evs,
                Event::AnimaGained {
                    seat: owner,
                    amount: *amount as u8,
                    reason: AnimaReason::Effect,
                },
            );
        }
        // The Toll: the engager's owner pays Anima (capped at what they have — the
        // "or the engage is spent" branch is the no-pay outcome, already the case).
        E::AnimaDelta { amount } if *amount < 0 && cl.selector == S::Engager => {
            if let Some(payer) = engager.and_then(|e| sim.spirit_at(e)).map(|s| s.owner) {
                let owed = (-*amount) as u8;
                let pay = owed.min(sim.player(payer).anima);
                if pay > 0 {
                    push(
                        sim,
                        evs,
                        Event::AnimaSpent {
                            seat: payer,
                            amount: pay,
                        },
                    );
                }
            }
        }
        // Harvest Together: 1 Anima per adjacent allied pair on the board (capped).
        E::AnimaPerAdjacentAlliedPair { max } if cl.selector == S::Owner => {
            let mut pairs = 0u8;
            for i in 0..sim.board.len() as u8 {
                for j in (i + 1)..sim.board.len() as u8 {
                    if manhattan(i, j) == 1
                        && sim
                            .spirit_at(i)
                            .map(|s| !s.fading && s.owner == owner)
                            .unwrap_or(false)
                        && sim
                            .spirit_at(j)
                            .map(|s| !s.fading && s.owner == owner)
                            .unwrap_or(false)
                    {
                        pairs += 1;
                    }
                }
            }
            let amount = pairs.min(*max);
            if amount > 0 {
                push(
                    sim,
                    evs,
                    Event::AnimaGained {
                        seat: owner,
                        amount,
                        reason: AnimaReason::Effect,
                    },
                );
            }
        }
        // Star-Strewn Otter: grant a one-shot discount on the owner's next Ritual.
        E::CostDelta { delta } if cl.selector == S::Owner && *delta < 0 => {
            push(
                sim,
                evs,
                Event::RitualDiscountGranted {
                    seat: owner,
                    amount: (-*delta) as u8,
                },
            );
        }
        // Ink Runs Dry: both Narrators' NEXT card costs more (a Solace tax, honored in
        // `cost_aura` for every play — spirit or Ritual). It is one-shot per seat (spent on
        // that seat's next play) and held through the next round so a player it taxes still
        // pays it on their following turn (the Solace plays it on its own turn).
        E::CostDelta { delta } if cl.selector == S::BothNarrators && *delta > 0 => {
            for seat in [Seat::A, Seat::B] {
                push(
                    sim,
                    evs,
                    Event::CardTaxed {
                        seat,
                        amount: *delta as u8,
                        until_round: sim.round + 1,
                    },
                );
            }
        }
        // Remember Them: draw 1 per ally that fully dissolved this turn (capped).
        E::DrawPerBanishedThisTurn { max } if cl.selector == S::Owner => {
            let n = sim.dissolved_this_turn[owner as usize].min(*max);
            for _ in 0..n {
                if !sim.player(owner).deck.is_empty() {
                    push(sim, evs, Event::CardDrawn { seat: owner });
                }
            }
        }
        // What Remains: 1 Anima per spirit you've lost this match (the dissolved pool).
        E::AnimaPerBanishedAlly { max } if cl.selector == S::Owner => {
            let lost = sim.dissolved.iter().filter(|(s, _)| *s == owner).count() as u8;
            let amount = lost.min(*max);
            if amount > 0 {
                push(
                    sim,
                    evs,
                    Event::AnimaGained {
                        seat: owner,
                        amount,
                        reason: AnimaReason::Effect,
                    },
                );
            }
        }
        E::RestoreForm { amount } => {
            // RestoreForm heals — and uniquely MAY target a fading ally (the
            // point of "restore" is to pull a dissolving spirit back). So for
            // ally selectors we resolve fading-inclusive, unlike other effects.
            let targets = restore_targets(sim, &cl.selector, source, owner, engager);
            for t in targets {
                push(
                    sim,
                    evs,
                    Event::EffectRestored {
                        tile: t,
                        amount: *amount,
                    },
                );
            }
        }
        E::Damage { amount } => {
            // EnemiesAdjacentToAlliesOf needs the catalog (resonance lookup): enemies
            // adjacent to any of the owner's spirits of that Resonance (Eruption).
            let targets: Vec<u8> = if let S::EnemiesAdjacentToAlliesOf { resonance } = &cl.selector
            {
                let res = *resonance;
                let sources: Vec<u8> = sim
                    .board
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        t.spirit
                            .as_ref()
                            .filter(|sp| {
                                !sp.fading
                                    && sp.owner == owner
                                    && card(catalog, sp.card).resonance == res
                            })
                            .map(|_| i as u8)
                    })
                    .collect();
                sim.board
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        t.spirit
                            .as_ref()
                            .filter(|sp| {
                                !sp.fading
                                    && sp.owner != owner
                                    && sources.iter().any(|&s| manhattan(s, i as u8) == 1)
                            })
                            .map(|_| i as u8)
                    })
                    .collect()
            } else {
                effect_targets(sim, &cl.selector, source, owner, engager)
            };
            for t in targets {
                push(
                    sim,
                    evs,
                    Event::EffectDamaged {
                        tile: t,
                        amount: *amount,
                    },
                );
                let lethal = sim
                    .spirit_at(t)
                    .map(|sp| sp.hp <= 0 && !sp.fading)
                    .unwrap_or(false);
                if lethal {
                    banish_or_replace(sim, evs, catalog, t, owner);
                }
            }
        }
        E::StatDelta {
            attack,
            defense,
            form,
        } => {
            // AlliesWithImprint needs the catalog (imprint lookup) that effect_targets
            // lacks — resolve it here (War Roar: your Beast spirits this round).
            // BondedPair resolves to both endpoints of `source`'s Bond (Common Cause).
            let targets: Vec<u8> = if let S::AlliesWithImprint { imprint } = &cl.selector {
                sim.board
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        t.spirit
                            .as_ref()
                            .filter(|sp| {
                                !sp.fading
                                    && sp.owner == owner
                                    && !sp.traits_blanked(sim.round)
                                    && card(catalog, sp.card)
                                        .imprints
                                        .iter()
                                        .any(|im| im == imprint)
                            })
                            .map(|_| i as u8)
                    })
                    .collect()
            } else if cl.selector == S::BondedPair {
                // Common Cause: a bond's OnDefeat StatDelta buffs both endpoints.
                source
                    .and_then(|src| {
                        sim.bonds
                            .iter()
                            .find(|b| b.tile_a == src || b.tile_b == src)
                            .map(|b| vec![b.tile_a, b.tile_b])
                    })
                    .unwrap_or_default()
            } else {
                effect_targets(sim, &cl.selector, source, owner, engager)
            };
            for t in targets {
                match cl.duration {
                    // ThisRound expires at the end of this round; NextRound rides
                    // through the next too (Your Name, Misheard: "this round and next").
                    Duration::ThisRound | Duration::NextRound => {
                        let until = if cl.duration == Duration::NextRound {
                            sim.round + 1
                        } else {
                            sim.round
                        };
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
                    }
                    _ => push(
                        sim,
                        evs,
                        Event::EffectStat {
                            tile: t,
                            attack: *attack,
                            defense: *defense,
                            form: *form,
                        },
                    ),
                }
            }
        }
        E::Displace(crate::effects::Displacement::Push { tiles }) => {
            // Shove the targeted enemy away from the source (The Vanishing Point,
            // ill-intent shoves). Uses the width-aware, Steadfast-respecting helper.
            if let Some(src) = source {
                for t in effect_targets(sim, &cl.selector, source, owner, engager) {
                    push_away(sim, evs, catalog, src, t, *tiles, owner);
                }
            }
        }
        E::RevealFabrication => {
            // Flip every enemy face-down Fabrication face-up, publicly (Zenith's
            // and Cirrus's arrival). A terrain effect, so it scans the board for
            // the OTHER seat's Fabrications rather than going through the
            // spirit-oriented `effect_targets` (gated to `EnemiesAll` by
            // `supported_instant_clause`).
            let enemy = owner.other();
            let reveals: Vec<u8> = sim
                .board
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    t.terrain.as_ref().and_then(|terr| {
                        (terr.owner == enemy
                            && terr.face_down
                            && terr.kind == crate::state::TerrainKind::Fabrication)
                            .then_some(i as u8)
                    })
                })
                .collect();
            for t in reveals {
                push(sim, evs, Event::FabricationRevealed { tile: t });
                buff_on_fab_reveal(sim, evs, catalog);
            }
        }
        _ => {}
    }
}
