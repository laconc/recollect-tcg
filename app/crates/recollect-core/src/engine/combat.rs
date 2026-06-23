//! Combat: the arrival/interception/momentum exchange and its forecast.
//! A sibling of `engine.rs`; `use super::*` pulls the shared engine helpers
//! (combat_stats, eff_defense, manhattan, …) and crate types.
use super::*;

pub(crate) fn pack_tactics_chip(
    sim: &GameState,
    catalog: &[CardDef],
    att_tile: u8,
    def_tile: u8,
) -> Option<i16> {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let b = sim
        .bonds
        .iter()
        .find(|b| b.tile_a == att_tile || b.tile_b == att_tile)?;
    if manhattan(b.tile_a, b.tile_b) != 1
        || !sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
        || !sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
    {
        return None;
    }
    let partner = if b.tile_a == att_tile {
        b.tile_b
    } else {
        b.tile_a
    };
    let name = &card(catalog, b.card).name;
    let chip = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(name.as_str()))?
        .iter()
        .filter(|s| s.trigger == Trigger::Static)
        .flat_map(|s| &s.clauses)
        .find_map(|cl| match &cl.effect {
            E::GrantEngage { pre_chip, .. } if *pre_chip > 0 && cl.selector == S::BondedPair => {
                Some(*pre_chip)
            }
            _ => None,
        })?;
    let psp = sim.spirit_at(partner)?;
    if psp.no_engage_until >= sim.round {
        return None; // Don't Look: this partner can't deal the proactive blow
    }
    let reach = card(catalog, psp.card).reach;
    if !oriented_w(reach, partner, psp.owner, sim.board_w).contains(&def_tile) {
        return None;
    }
    Some(chip)
}

/// Conspiracy: the spirit at `def_tile` is in a Bond whose GrantEngage is
/// `immediate` (a reactive counter-engage, not a pre-chip). When that spirit is
/// engaged and the pair is present & adjacent and the PARTNER has the attacker
/// (`att_tile`) in reach, the partner immediately counter-engages the attacker.
/// Returns `(partner_tile, partner_owner)`; the caller runs the engage + momentum.
pub(crate) fn conspiracy_counter(
    sim: &GameState,
    catalog: &[CardDef],
    def_tile: u8,
    att_tile: u8,
) -> Option<(u8, Seat)> {
    use crate::effects::{Effect as E, Selector as S, Trigger};
    let b = sim
        .bonds
        .iter()
        .find(|b| b.tile_a == def_tile || b.tile_b == def_tile)?;
    if manhattan(b.tile_a, b.tile_b) != 1
        || !sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
        || !sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false)
    {
        return None;
    }
    let partner = if b.tile_a == def_tile {
        b.tile_b
    } else {
        b.tile_a
    };
    let name = &card(catalog, b.card).name;
    let is_counter_bond = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(name.as_str()))?
        .iter()
        .filter(|s| s.trigger == Trigger::Static)
        .flat_map(|s| &s.clauses)
        .any(|cl| {
            cl.selector == S::BondedPair
                && matches!(
                    &cl.effect,
                    E::GrantEngage {
                        immediate: true,
                        ..
                    }
                )
        });
    if !is_counter_bond {
        return None;
    }
    let psp = sim.spirit_at(partner)?;
    if psp.no_engage_until >= sim.round {
        return None; // Don't Look: this partner can't deal the proactive blow
    }
    let reach = card(catalog, psp.card).reach;
    if !oriented_w(reach, partner, psp.owner, sim.board_w).contains(&att_tile) {
        return None;
    }
    Some((partner, psp.owner))
}

/// Grief Split / Promise: a Bond that moves (part of) a lethal blow from the
/// bonded spirit at `def_tile` to its partner. Returns `(partner, redirect, consume)`
/// when the blow `dmg` would be lethal (the spirit is at `hp`):
/// - **Promise** (`Replace(PartnerTakesIt)`, OncePerMatch): a FULL redirect — the
///   partner takes the whole blow, the saved spirit is untouched, the Promise is
///   spent (`consume == true`). Honors `replacement_used` on the saved spirit.
/// - **Grief Split** (`RedirectDamageToPartner`, PairAdjacent): the lethal OVERFLOW
///   is borne by the partner — the bonded spirit survives at 1 HP, the partner
///   absorbs `dmg - (hp - 1)`. Repeatable (`consume == false`).
///
/// Same-owner bonds, so the partner is never the (enemy) attacker.
pub(crate) fn damage_redirect_to_partner(
    sim: &GameState,
    catalog: &[CardDef],
    def_tile: u8,
    hp: i16,
    dmg: i16,
) -> Option<(u8, i16, bool)> {
    use crate::effects::{Condition, Effect as E, Replacement, Trigger};
    if dmg < hp {
        return None; // not lethal — the ordinary strike stands
    }
    let b = sim
        .bonds
        .iter()
        .find(|b| b.tile_a == def_tile || b.tile_b == def_tile)?;
    let partner = if b.tile_a == def_tile {
        b.tile_b
    } else {
        b.tile_a
    };
    if !sim.spirit_at(partner).map(|s| !s.fading).unwrap_or(false) {
        return None; // no standing partner to bear it
    }
    let name = card(catalog, b.card).name.clone();
    let specs = crate::effects::canon_effects()
        .specs
        .get(crate::cards::key_of(&name))?;
    let saved_used = sim
        .spirit_at(def_tile)
        .map(|s| s.replacement_used)
        .unwrap_or(true);
    for spec in specs {
        if spec.trigger != Trigger::Static {
            continue;
        }
        for cl in &spec.clauses {
            match &cl.effect {
                E::Replace(Replacement::PartnerTakesIt)
                    if spec.condition == Condition::OncePerMatch && !saved_used =>
                {
                    return Some((partner, dmg, true)); // Promise: full redirect, spent
                }
                E::RedirectDamageToPartner
                    if manhattan(b.tile_a, b.tile_b) == 1
                        && sim.spirit_at(b.tile_a).map(|s| !s.fading).unwrap_or(false)
                        && sim.spirit_at(b.tile_b).map(|s| !s.fading).unwrap_or(false) =>
                {
                    return Some((partner, dmg - (hp - 1), false)); // overflow; bonded survives at 1
                }
                _ => {}
            }
        }
    }
    None
}

pub(crate) fn full_exchange(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    att_tile: u8,
    def_tile: u8,
    kind: StrikeKind,
    bonus: i16,
) -> bool {
    if sim
        .spirit_at(def_tile)
        .map(|s| s.face_down)
        .unwrap_or(false)
    {
        push(sim, evs, Event::SpiritRevealed { tile: def_tile });
        forced_reveal_effects(sim, evs, &ctx.catalog, def_tile);
    }
    // Pack Tactics: when a bonded spirit ENGAGES, its partner first chips the target
    // (if the partner has it in reach). Resolved before the strike, so the exchange
    // reads the chipped HP. If the chip alone fells the target, the engage is done.
    if matches!(kind, StrikeKind::Engage)
        && let Some(chip) = pack_tactics_chip(sim, &ctx.catalog, att_tile, def_tile)
    {
        push(
            sim,
            evs,
            Event::EffectDamaged {
                tile: def_tile,
                amount: chip,
            },
        );
        if sim
            .spirit_at(def_tile)
            .map(|s| s.hp <= 0 && !s.fading)
            .unwrap_or(false)
        {
            let by = sim.spirit_at(att_tile).map(|s| s.owner).unwrap_or(Seat::A);
            banish_or_replace(sim, evs, &ctx.catalog, def_tile, by);
            return true;
        }
    }
    // The forced reveal (Madrigal's push, a trap's bounce) may displace either
    // party off its tile before the strike resolves — then there is no
    // exchange to compute. The arrival simply stands where it landed.
    let (Some(att), Some(dfn)) = (
        sim.spirit_at(att_tile).cloned(),
        sim.spirit_at(def_tile).cloned(),
    ) else {
        return false;
    };
    let ac = card(&ctx.catalog, att.card).clone();
    let dc = card(&ctx.catalog, dfn.card).clone();
    // The Lullaby: a spirit adjacent to an enemy Lullaby is too calm to Echo.
    let a_echo = att.echo_eligible()
        && !echo_suppressed(sim, &ctx.catalog, att_tile)
        && ctx.entropy.draw_below(ECHO_NUM, ECHO_DEN);
    let d_echo = dfn.echo_eligible()
        && !echo_suppressed(sim, &ctx.catalog, def_tile)
        && ctx.entropy.draw_below(ECHO_NUM, ECHO_DEN);
    // Derived combat numbers (temp mods, auras, Attune, edges).
    let acs = combat_stats(sim, &ctx.catalog, att_tile, Some(def_tile));
    let dcs = combat_stats(sim, &ctx.catalog, def_tile, Some(att_tile));
    let ares = acs.resonance.unwrap_or(ac.resonance);
    let dres = dcs.resonance.unwrap_or(dc.resonance);
    let a_edge = if ares.edge_over(dres) && !acs.edge_negated_against {
        EDGE
    } else {
        0
    };
    let d_edge = if dres.edge_over(ares) && !dcs.edge_negated_against {
        EDGE
    } else {
        0
    };
    // Chain context: Badgermarshal reduces chain damage to the defender (its ally);
    // Pyrrhic takes no retaliation on its chain links from the 2nd on.
    let chain_link = if let StrikeKind::Chain(l) = kind {
        l
    } else {
        0
    };
    let chain_reduction = if chain_link > 0 {
        chain_damage_reduction(sim, &ctx.catalog, def_tile)
    } else {
        0
    };
    // The Unforgiving: its (arcane) strikes ignore the defender's Warded.
    let def_warded = eff_warded(sim, &ctx.catalog, def_tile)
        && !card_carries_static_exception(
            &ctx.catalog,
            att.card,
            crate::effects::RuleException::StrikesIgnoreWarded,
        );
    let dmg_def = (acs.atk + a_edge + bonus + if a_echo { ECHO_BONUS } else { 0 }
        - eff_defense(dcs.def, ac.arcane, def_warded)
        - dcs.dmg_reduction
        - chain_reduction)
        .max(0);
    // Retaliation needs no Reach — memory, not ballistics.
    let dmg_att = if chain_link >= 2 && acs.chain_no_retaliation {
        0
    } else {
        (dcs.atk + dcs.retaliation + d_edge + if d_echo { ECHO_BONUS } else { 0 }
            - eff_defense(acs.def, dc.arcane, ac.warded)
            - acs.dmg_reduction)
            .max(0)
    };
    // Grief Split / Promise: a bonded spirit's partner bears (part of) a lethal
    // blow. Resolve the redirect BEFORE the strike so the defender's HP reflects
    // only what it keeps, and the partner takes (and may be banished by) the rest.
    let mut dmg_def = dmg_def;
    // The Forgiven Debt: a lethal blow from an attacker at Echo is capped to leave it at 1 HP
    // (matches the forecast). Applied before the Grief redirect so it sees the kept HP.
    if att.echo_eligible()
        && card_carries_static_exception(
            &ctx.catalog,
            dfn.card,
            crate::effects::RuleException::UnbanishableByEcho,
        )
    {
        dmg_def = dmg_def.min((dfn.hp - 1).max(0));
    }
    if let Some((partner, redirect, consume)) =
        damage_redirect_to_partner(sim, &ctx.catalog, def_tile, dfn.hp, dmg_def)
        && redirect > 0
    {
        let p_pre = sim.spirit_at(partner).map(|s| s.hp).unwrap_or(0);
        push(
            sim,
            evs,
            Event::DamageRedirected {
                from_tile: def_tile,
                to_tile: partner,
                amount: redirect,
                consume,
            },
        );
        if p_pre - redirect <= 0 {
            banish_or_replace(sim, evs, &ctx.catalog, partner, att.owner);
        }
        dmg_def -= redirect;
    }
    push(
        sim,
        evs,
        Event::Struck {
            from_tile: att_tile,
            to_tile: def_tile,
            damage: dmg_def,
            echo: a_echo,
            kind,
        },
    );
    // Warden-Breaker: capture the defender's Warded status before it leaves play.
    let def_warded = eff_warded(sim, &ctx.catalog, def_tile);
    let mut banished = dfn.hp - dmg_def <= 0;
    if banished {
        banished = banish_or_replace(sim, evs, &ctx.catalog, def_tile, att.owner);
        // OnDefeat fires for a still-standing victor.
        if banished && att.hp - dmg_att > 0 {
            let name = ac.name.clone();
            fire_effects_noctx(
                sim,
                evs,
                &ctx.catalog,
                &name,
                crate::effects::Trigger::OnDefeat,
                Some(att_tile),
                att.owner,
            );
            // Warden-Breaker, Crowned in Smoke: defeating a WARDED enemy buffs the victor.
            if def_warded {
                fire_self_stat_buff(
                    sim,
                    evs,
                    &ctx.catalog,
                    att_tile,
                    crate::effects::Trigger::OnDefeatWarded,
                );
            }
            // A bonded victor also fires its BOND's OnDefeat (Common Cause buffs the pair).
            if let Some(bcard) = sim
                .bonds
                .iter()
                .find(|b| b.tile_a == att_tile || b.tile_b == att_tile)
                .map(|b| b.card)
            {
                let bname = card(&ctx.catalog, bcard).name.clone();
                fire_effects_noctx(
                    sim,
                    evs,
                    &ctx.catalog,
                    &bname,
                    crate::effects::Trigger::OnDefeat,
                    Some(att_tile),
                    att.owner,
                );
            }
        }
    }
    push(
        sim,
        evs,
        Event::Struck {
            from_tile: def_tile,
            to_tile: att_tile,
            damage: dmg_att,
            echo: d_echo,
            kind: StrikeKind::Retaliation,
        },
    );
    // Read the attacker's banish off the LIVE board, not the pre-exchange `att.hp` snapshot:
    // the retaliation Struck above already landed, and any earlier same-tile mutation in this
    // exchange is reflected too, so a standing attacker driven to ≤0 here is always banished.
    // (The snapshot `att.hp - dmg_att` matches on every reachable path today — Bonds break
    // before a chain can redirect onto the engager (`prune_broken_bonds`) — but reading the
    // live, still-standing HP keeps the "no standing spirit at ≤0 HP" invariant true under any
    // future mid-exchange wound, while never firing on a tile a displacement already vacated.)
    let att_down = sim
        .spirit_at(att_tile)
        .map(|s| !s.fading && s.hp <= 0)
        .unwrap_or(false);
    if att_down {
        banish_or_replace(sim, evs, &ctx.catalog, att_tile, dfn.owner);
    } else if sim.spirit_at(def_tile).map(|s| !s.fading).unwrap_or(false) {
        // The defender stands (survived or was Replaced): its on-engaged
        // effects fire with the engager in context (Patient Arbiter).
        let dname = dc.name.clone();
        fire_engaged(
            sim,
            evs,
            &ctx.catalog,
            &dname,
            def_tile,
            dfn.owner,
            att_tile,
        );
    }
    // The attacker's OnEngageResolved (Vertigo pushes the survivor).
    if sim.spirit_at(att_tile).map(|s| !s.fading).unwrap_or(false) {
        let aname = ac.name.clone();
        fire_survivor(
            sim,
            evs,
            &ctx.catalog,
            &aname,
            att_tile,
            att.owner,
            att_tile,
            def_tile,
            banished,
        );
    }
    // Conspiracy: a freshly-engaged bonded spirit (still standing, pair adjacent)
    // lets its partner immediately counter-engage the attacker — a full engage
    // (with its own momentum). The ctx guard prevents a counter triggering another.
    if matches!(kind, StrikeKind::Engage)
        && !ctx.conspiracy_active
        && sim.spirit_at(att_tile).map(|s| !s.fading).unwrap_or(false)
        && let Some((partner, p_owner)) = conspiracy_counter(sim, &ctx.catalog, def_tile, att_tile)
    {
        ctx.conspiracy_active = true;
        full_exchange(sim, evs, ctx, partner, att_tile, StrikeKind::Engage, 0);
        momentum(sim, evs, ctx, partner, p_owner);
        ctx.conspiracy_active = false;
    }
    banished
}

/// Interception (capped): one per arrival, defender's best eligible spirit,
/// once per spirit per round; a single strike — no retaliation, no chain.
/// Engine policy: auto-intercept with the highest-Attack eligible coverer.
/// The Arrival Law's middle step: when a spirit arrives at `arrival`, the
/// best-placed enemy defender in reach (not Held, not face-down, not yet
/// intercepted this round, not on a dark rim) bites it once. A Feral Stray
/// also bites (see `feral_stray_intercepts`). Standing Orders and the Held
/// Ground law gate who may intercept.
pub(crate) fn interception(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    arrival: u8,
    actor: Seat,
) {
    let arr = match sim.spirit_at(arrival) {
        Some(sp) if !sp.fading => sp.clone(),
        _ => return,
    };
    // The Closing Book: immune to interception on arrival (the page shuts over it).
    if card_carries_static_exception(
        &ctx.catalog,
        arr.card,
        crate::effects::RuleException::ImmuneToInterception,
    ) {
        return;
    }
    let defender = actor.other();
    let arr_card = card(&ctx.catalog, arr.card).clone();
    let mut best: Option<(u8, i16)> = None; // (tile, attack)
    for (i, t) in sim.board.iter().enumerate() {
        if let Some(sp) = &t.spirit
            && sp.owner == defender
                && !sp.fading
                && !sp.holding // Standing Orders: Held never intercepts
                && !sp.face_down // hidden Lurkers do not intercept (Arrival Law)
                && !sp.intercepted_this_round
                && sp.no_engage_until < sim.round // Don't Look: can't intercept
                // The Held Ground law: held spirits retaliate if struck,
                // but their zones no longer bite.
                && !(sim.contracted && is_rim_w(i as u8, sim.board_w))
        {
            let c = card(&ctx.catalog, sp.card);
            // The Almost-Said intercepts with the Reach it copied, if any.
            let reach = sp.copied_reach.unwrap_or(c.reach);
            // A FULL reach buff (Tailwind, Open Sky) widens the interceptor's bite.
            let (fwd, ad) = full_reach_delta(sim, i as u8, defender);
            let bite = widen_oriented(
                oriented(reach, i as u8, defender),
                fwd,
                ad,
                defender,
                sim.board_w,
            );
            if bite.contains(&arrival) {
                let better = match best {
                    None => true,
                    Some((_, a)) => sp.attack > a,
                };
                if better {
                    best = Some((i as u8, sp.attack));
                }
            }
        }
    }
    let Some((it, _)) = best else { return };
    let icp = sim.spirit_at(it).cloned().unwrap();
    let ic = card(&ctx.catalog, icp.card).clone();
    let echo_possible = icp.echo_eligible() && !echo_suppressed(sim, &ctx.catalog, it);
    let edge = if ic.resonance.edge_over(arr_card.resonance) {
        EDGE
    } else {
        0
    };
    // The Unforgiving: even its interception bite ignores the arriver's Warded.
    let arr_warded = eff_warded(sim, &ctx.catalog, arrival)
        && !card_carries_static_exception(
            &ctx.catalog,
            icp.card,
            crate::effects::RuleException::StrikesIgnoreWarded,
        );
    let base = (icp.attack + edge - eff_defense(arr.defense, ic.arcane, arr_warded)).max(0);
    if base == 0 && !echo_possible {
        return; // nothing to bite with
    }
    let echo = echo_possible && ctx.entropy.draw_below(ECHO_NUM, ECHO_DEN);
    let dmg = base + if echo { ECHO_BONUS } else { 0 };
    push(
        sim,
        evs,
        Event::Struck {
            from_tile: it,
            to_tile: arrival,
            damage: dmg,
            echo,
            kind: StrikeKind::Interception,
        },
    );
    if arr.hp - dmg <= 0 {
        banish_or_replace(sim, evs, &ctx.catalog, arrival, defender);
    }
    feral_stray_intercepts(sim, evs, ctx, arrival, actor);
}

/// Feral: a Feral Stray defends its tile — it intercepts an arrival in its
/// reach, biting the newcomer; the arrival wounds it back in the exchange,
/// which is how a Feral Stray is brought below half HP (its Echo) and finally
/// becomes befriendable. Gentle/Wary Strays do not fight.
pub(crate) fn feral_stray_intercepts(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    arrival: u8,
    actor: Seat,
) {
    let Some(stray) = sim.stray.clone() else {
        return;
    };
    if stray.veiled || stray.temperament != crate::state::Temperament::Feral {
        return;
    }
    let arr = match sim.spirit_at(arrival) {
        Some(sp) if !sp.fading && sp.owner == actor => sp.clone(),
        _ => return,
    };
    let sc = card(&ctx.catalog, stray.card).clone();
    if !oriented_w(sc.reach, stray.tile, actor.other(), sim.board_w).contains(&arrival) {
        return;
    }
    let arr_card = card(&ctx.catalog, arr.card).clone();
    // The Stray bites the arrival.
    let bite = (sc.attack - eff_defense(arr.defense, sc.arcane, arr_card.warded)).max(0);
    if bite > 0 {
        push(
            sim,
            evs,
            Event::Struck {
                from_tile: stray.tile,
                to_tile: arrival,
                damage: bite,
                echo: false,
                kind: StrikeKind::Interception,
            },
        );
        if sim
            .spirit_at(arrival)
            .map(|s| s.hp - bite <= 0)
            .unwrap_or(false)
        {
            banish_or_replace(sim, evs, &ctx.catalog, arrival, actor);
        }
    }
    // The arrival wounds the Stray back (this is the Echo path).
    let counter = (arr.attack - eff_defense(sc.defense, arr_card.arcane, sc.warded)).max(0);
    if counter > 0 {
        if let Some(s) = sim.stray.as_mut() {
            s.hp -= counter;
        }
        let (tile, new_hp) = match &sim.stray {
            Some(s) => (s.tile, s.hp),
            None => return,
        };
        push(
            sim,
            evs,
            Event::StrayStruck {
                tile,
                damage: counter,
                hp: new_hp.max(0),
            },
        );
        if new_hp <= 0 {
            sim.stray = None;
            push(
                sim,
                evs,
                Event::StrayBanished {
                    tile,
                    impression: actor,
                },
            );
        }
    }
}

/// Momentum, inverted: one bonus engagement at +10; Relentless chains
/// while defeats continue. Each link is a full exchange (retaliation lives).
/// Engine policy: auto-target banishing-first then highest HP (interactive
/// choice is a red test).
/// The Arrival Law's last step: a spirit that defeated its target on arrival
/// may get one bonus engagement (base Momentum); Relentless chains while
/// defeats continue. Fires only after a kill on the arrival exchange.
/// The Arrival Law's last step: a spirit that defeated its target on arrival
/// may get one bonus engagement (base Momentum); Relentless chains while
/// defeats continue. Fires only after a kill on the arrival exchange.
///
/// `prefs` is the ordered chain-target preference list from the arrival
/// command: each link takes the first preferred target that is currently a
/// legal enemy in reach; if none of the preferences apply, the engine falls
/// back to its banishing-first heuristic. Deterministic and async-safe — the
/// engine never waits on a human mid-resolution.
pub(crate) fn momentum_prefs(
    sim: &mut GameState,
    evs: &mut Vec<Event>,
    ctx: &mut TurnCtx,
    tile: u8,
    actor: Seat,
    prefs: &[u8],
) {
    let mut link: i16 = 1;
    loop {
        let sp = match sim.spirit_at(tile) {
            Some(sp) if !sp.fading => sp.clone(),
            _ => return,
        };
        let def = card(&ctx.catalog, sp.card).clone();
        let cs = combat_stats(sim, &ctx.catalog, tile, None);
        if link > 1 && !def.relentless && !cs.chain_while_defeating {
            return;
        }
        let bonus = (MOMENTUM_PER_LINK + cs.momentum_per_link_bonus)
            * (link
                + if cs.momentum_first_bonus && link == 1 {
                    1
                } else {
                    0
                });
        let targets: Vec<u8> =
            targeting_reach(sim, &ctx.catalog, def.reach, tile, actor, sim.board_w)
                .into_iter()
                .filter(|t| matches!(sim.spirit_at(*t), Some(e) if e.owner != actor && !e.fading))
                .collect();
        if targets.is_empty() {
            return;
        }
        // take the first preferred target that is a legal enemy in reach;
        // else fall back to the banishing-first heuristic.
        let pick = prefs
            .iter()
            .copied()
            .find(|p| targets.contains(p))
            .unwrap_or_else(|| {
                targets
                    .iter()
                    .copied()
                    .max_by_key(|t| {
                        let e = sim.spirit_at(*t).unwrap();
                        let ec = card(&ctx.catalog, e.card);
                        let banishing = e.hp
                            <= (sp.attack
                                + bonus
                                + if def.resonance.edge_over(ec.resonance) {
                                    EDGE
                                } else {
                                    0
                                }
                                - eff_defense(
                                    e.defense,
                                    def.arcane,
                                    eff_warded(sim, &ctx.catalog, *t),
                                ))
                            .max(0);
                        (banishing as i16, e.hp)
                    })
                    .unwrap()
            });
        let banished = full_exchange(
            sim,
            evs,
            ctx,
            tile,
            pick,
            StrikeKind::Chain(link as u8),
            bonus,
        );
        if !banished {
            return;
        }
        link += 1;
    }
}

/// Pure exchange forecast — the single source of the numbers every client
/// shows before a commitment (the forecast pillar: combat is fully forecast).
/// Echo is reported as eligibility, never rolled: variance is seeded,
/// visible, and merciful — the forecast tells you whose dice are live.
pub struct Forecast {
    pub to_defender: i16,
    pub to_attacker: i16,
    pub banishes_defender: bool,
    pub banishes_attacker: bool,
    pub attacker_echo_live: bool,
    pub defender_echo_live: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn forecast_exchange(
    ac: &CardDef,
    att_attack: i16,
    att_defense: i16,
    att_hp: i16,
    att_hp_max: i16,
    dfn: &Spirit,
    dc: &CardDef,
    bonus: i16,
    // The defender's EFFECTIVE Warded (intrinsic OR aura-granted); the caller computes it
    // from board state so the preview matches `full_exchange` (which routes the same way).
    def_warded: bool,
) -> Forecast {
    let a_edge = if ac.resonance.edge_over(dc.resonance) {
        EDGE
    } else {
        0
    };
    let d_edge = if dc.resonance.edge_over(ac.resonance) {
        EDGE
    } else {
        0
    };
    let to_defender =
        (att_attack + a_edge + bonus - eff_defense(dfn.defense, ac.arcane, def_warded)).max(0);
    // The Forgiven Debt: a lethal blow from an attacker at Echo is capped to leave it at 1 HP.
    let to_defender = if att_hp * 2 <= att_hp_max
        && name_carries_static_exception(
            &dc.name,
            crate::effects::RuleException::UnbanishableByEcho,
        ) {
        to_defender.min((dfn.hp - 1).max(0))
    } else {
        to_defender
    };
    let to_attacker = (dfn.attack + d_edge - eff_defense(att_defense, dc.arcane, ac.warded)).max(0);
    Forecast {
        to_defender,
        to_attacker,
        banishes_defender: dfn.hp - to_defender <= 0,
        banishes_attacker: att_hp - to_attacker <= 0,
        attacker_echo_live: att_hp * 2 <= att_hp_max,
        defender_echo_live: dfn.echo_eligible(),
    }
}
