//! Effect-IR support predicates: which Triggers/Conditions/Selectors/Effects the engine
//! actually executes — the `card_fully_supported` coverage gate (`effects_coverage.rs`).
//! A sibling of `effects.rs`; `use super::*` pulls the IR enums.
use super::*;

/// Choice-free clauses the engine executes directly. Choice-bearing selectors
/// and modifier durations are resolved elsewhere.
pub fn supported_instant_clause(c: &Clause) -> bool {
    // A Bluff that genuinely does nothing on reveal — the no-op IS the card, because
    // the engager already spent its strike: Nothing, Really (Owner) and Paper Sentinel
    // (Engager — "the engage is spent" is descriptive, not a new mechanic).
    if matches!(c.effect, Effect::NoEffect) {
        return matches!(c.selector, Selector::Owner | Selector::Engager);
    }
    // The Toll: the engager's owner pays Anima when the lie springs (Engager/AnimaDelta).
    if matches!(c.effect, Effect::AnimaDelta { .. }) && c.selector == Selector::Engager {
        return true;
    }
    // The Provocation: the engager is forced into one more engage at −10 (honored in
    // spring_fabrication). Ragewoken Bison: SelfSpirit/ExtraEngage gated by PayForm —
    // pay HP at OnPlay to engage an extra chosen target.
    if matches!(c.effect, Effect::ExtraEngage)
        && matches!(c.selector, Selector::Engager | Selector::SelfSpirit)
    {
        return true;
    }
    // Bearer of Small Stones: a Parting frees this turn's evolutions from the
    // shared-Imprint rule (sets a transient flag the evolution check consults).
    if matches!(
        c.effect,
        Effect::Exception(RuleException::EvolveIgnoresSharedImprint)
    ) {
        return c.selector == Selector::Owner;
    }
    // Solace / antagonist board mutations: each is realized by a dedicated executor arm in
    // `exec_clause_mode` (the early-return path that precedes the generic shapes below).
    // The Page Turns / A Mercy for the Rim / Silence Spreads / Lost Paragraph / Let It Lie /
    // The Quiet Spreads are Owner-scoped Unwriting events.
    if matches!(
        c.effect,
        Effect::ShiftUnwrittenInward
            | Effect::ReleaseHeldRim
            | Effect::SilenceLandmark
            | Effect::ManifestUnwrittenOnRim
            | Effect::StopImpressionScoring
            | Effect::ScheduleCalmTiles
    ) {
        return c.selector == Selector::Owner;
    }
    // The Devouring Margin: OnMove ImpressionEat (realized on the inward shift — it eats the
    // player's mark it lands on; its paired RestoreForm heal rides the generic SelfSpirit arm).
    if matches!(c.effect, Effect::ImpressionEat) {
        return c.selector == Selector::SelfSpirit;
    }
    // Footnote / Sentence Fragment: on Unwrite, mill a deck's top card (SelfSpirit; the
    // `opponent` flag picks the seat).
    if matches!(c.effect, Effect::MillTopDeck { .. }) {
        return c.selector == Selector::SelfSpirit;
    }
    // The Half-Remembered (CopyLastPlayed), The Almost-Said (CopyEngagerReach), The
    // Misremembered (CopyPrintedStats): a SelfSpirit copy fired on reveal / engage-resolved.
    if matches!(
        c.effect,
        Effect::CopyLastPlayed | Effect::CopyEngagerReach | Effect::CopyPrintedStats
    ) {
        return c.selector == Selector::SelfSpirit;
    }
    // Ink Runs Dry: both Narrators' next card costs more this round (BothNarrators/CostDelta>0,
    // honored in cost_aura via the per-seat card_tax).
    if let Effect::CostDelta { delta } = c.effect
        && c.selector == Selector::BothNarrators
    {
        return delta > 0;
    }
    // Smear: the engaged enemy loses its printed Traits this round (EngagedEnemy/TraitStrip,
    // a round-scoped blank via exec_trait_strip). The Blank Page's permanent Engager/TraitStrip
    // is credited by the Fabrication-trap arm below.
    if matches!(c.effect, Effect::TraitStrip) && c.selector == Selector::EngagedEnemy {
        return true;
    }
    // Choice selectors open pending phases (own turn) or resolve
    // by doctrine (Parting); peeks and pushes are executed shapes.
    if matches!(
        c.selector,
        Selector::AdjacentAllyChoose | Selector::AdjacentEnemyChoose
    ) {
        return matches!(
            c.effect,
            Effect::RestoreForm { .. }
                | Effect::StatDelta { .. }
                | Effect::Displace(Displacement::Push { .. })
                | Effect::Damage { .. }
                | Effect::Release
                | Effect::Banish
                // What You Set Down: bounce a chosen adjacent enemy to hand
                // (doctrine-resolved via exec_adjacent_ally_choose).
                | Effect::Bounce
        );
    }
    // Free target-choice (Hold Fast; Salt and Honey / Mend; Stoke): a played card
    // picks ANY spirit board-wide via PendingChoice::Target. StatDelta + RestoreForm
    // + Damage — other target effects (GrantKeyword, Displace) await their executors.
    if matches!(
        c.selector,
        Selector::TargetSpirit | Selector::TargetAllySpirit | Selector::TargetEnemySpirit
    ) {
        return matches!(
            c.effect,
            Effect::StatDelta { .. }
                | Effect::RestoreForm { .. }
                | Effect::Damage { .. }
                | Effect::GrantKeyword { .. }
                // Two-step displacement: a played card slides the chosen spirit
                // (Quiet Step / Misstep / Cold Spot). Resolved via DisplaceFrom→MoveTo.
                | Effect::Displace(Displacement::MoveAny { .. } | Displacement::Push { .. })
                // Behind You: pick an enemy, then swap it with an adjacent enemy.
                | Effect::Displace(Displacement::Swap)
                // The Fog of Elsewhere: return the chosen (Cost-capped) enemy to hand.
                | Effect::Bounce
                // Sudden Clearing: reveal a chosen face-down Fabrication (the target
                // seam offers fabrication tiles, resolved via RevealFabricationAt).
                | Effect::RevealFabrication
                // Don't Look: the chosen enemy can't engage or intercept next round
                // (honored at interception + the move/reveal-engage gates).
                | Effect::Restrict(Restriction::Engage)
                // Tailwind: a per-spirit FULL reach buff this round.
                | Effect::ReachDelta { .. }
                // Reckless Charge: the chosen ally engages now + a retaliation shift.
                | Effect::GrantEngage { immediate: true, .. }
                | Effect::RetaliationDelta { .. }
        ) && matches!(
            c.duration,
            Duration::Instant | Duration::ThisRound | Duration::Permanent | Duration::NextRound
        );
    }
    // Hold the Memory: a played card delays a chosen Fading spirit's dissolution.
    if c.selector == Selector::TargetFadingSpirit {
        return matches!(c.effect, Effect::Replace(Replacement::DelayFadeOneStep));
    }
    // Kindle (StatDelta) / Again! (SecondTargetEngage): a buff for the seat's next
    // arriving spirit this turn.
    if c.selector == Selector::NextArrivalThisTurn {
        return matches!(
            c.effect,
            Effect::StatDelta { .. } | Effect::SecondTargetEngage { .. }
        );
    }
    // Round / Close Ranks: buff two adjacent allies (dependent pick-then-adjacent).
    if c.selector == Selector::TargetTwoAdjacentAllies {
        return matches!(c.effect, Effect::StatDelta { .. })
            && matches!(
                c.duration,
                Duration::Instant | Duration::ThisRound | Duration::Permanent
            );
    }
    // The Vanishing Point / ill-intent shoves: push (or strike, or weaken) the enemy
    // the source engaged.
    if matches!(c.selector, Selector::EngagedEnemy) {
        return matches!(
            c.effect,
            Effect::Displace(Displacement::Push { .. })
                | Effect::StatDelta { .. }
                | Effect::Damage { .. }
        );
    }
    if matches!(c.effect, Effect::PeekDeck { .. }) {
        return c.selector == Selector::Owner;
    }
    // Public reveal of every enemy face-down Fabrication (Zenith, Cirrus). Only
    // the board-wide `EnemiesAll` form is wired; the single-target "reveal this"
    // and the private "look at" forms (Curio Fox, What's That?) are not.
    if matches!(c.effect, Effect::RevealFabrication) {
        // EnemiesAll = board-wide public reveal (Zenith/Cirrus); TargetSpirit* = a chosen
        // public reveal (Sudden Clearing); Owner = a PRIVATE peek of one (Curio Fox).
        return matches!(
            c.selector,
            Selector::EnemiesAll
                | Selector::Owner
                | Selector::TargetSpirit
                | Selector::TargetAllySpirit
                | Selector::TargetEnemySpirit
        );
    }
    if matches!(c.effect, Effect::Displace(Displacement::Push { .. })) {
        // Board-wide adjacent shove, OR a Fabrication trap shoving its engager
        // back (Bottomless Puddle / Standing Stones / Short Fuse), OR Vertigo pushing
        // the survivor of its engage (fire_survivor). All resolve via `push_away`.
        return matches!(
            c.selector,
            Selector::AdjacentEnemiesAll | Selector::Engager | Selector::Survivor
        );
    }
    // Bounce: a Fabrication trap returns its engager to hand (The Unasked Question).
    if matches!(c.effect, Effect::Bounce) {
        return c.selector == Selector::Engager;
    }
    // TakeControl: a Fabrication trap steals its engager (The Perfect Lie).
    if matches!(c.effect, Effect::TakeControl) {
        return c.selector == Selector::Engager;
    }
    // TraitStrip: a Fabrication trap silences its engager's keywords/traits (The Blank Page).
    if matches!(c.effect, Effect::TraitStrip) {
        return c.selector == Selector::Engager;
    }
    // Seat-wide reach buff (Tempestrider Roc &c. targeting-only; Open Sky FULL —
    // a full buff also widens projection + interception via `full_reach_delta`).
    // Recorded as a `ReachBuffed`. (Per-spirit Tailwind goes through the free-target
    // seam below.)
    if matches!(c.effect, Effect::ReachDelta { .. }) && c.selector == Selector::AlliesAll {
        return c.duration == Duration::ThisRound;
    }
    // This-round seat-wide movement/push restriction (Stand Ground): recorded as a
    // `Restricted` and honored at the move + push call sites via `restricted()`.
    if matches!(
        c.effect,
        Effect::Restrict(Restriction::BePushed | Restriction::Move)
    ) {
        return c.selector == Selector::AlliesAll && c.duration == Duration::ThisRound;
    }
    // Draw: both narrators (Trade Winds) or just the owner (Gather In, Open Sky).
    if matches!(c.effect, Effect::Draw { .. }) {
        return matches!(c.selector, Selector::BothNarrators | Selector::Owner);
    }
    // Recover: open a choice over the owner's dissolved spirits (The Returning).
    if matches!(c.effect, Effect::Recover) {
        return c.selector == Selector::Owner;
    }
    // Scrub the Margin: open a choice over enemy impressions to erase one.
    if matches!(c.effect, Effect::ImpressionRemoveTarget) {
        return c.selector == Selector::Owner;
    }
    // Star-Strewn Otter: a one-shot Ritual discount (instant Owner/CostDelta).
    if matches!(c.effect, Effect::CostDelta { .. }) {
        return c.selector == Selector::Owner;
    }
    // The Long Walk: a Parting heals the dissolving spirit's bonded partner.
    if matches!(c.selector, Selector::BondedPartner) {
        return matches!(c.effect, Effect::RestoreForm { .. });
    }
    // Hold the Note: pick one of your Bonds; restore both endpoints.
    if matches!(c.selector, Selector::TargetBondedPair) {
        return matches!(c.effect, Effect::RestoreForm { .. });
    }
    // Common Cause: a bond's OnDefeat StatDelta buffs both endpoints (the executor
    // resolves BondedPair to the victor's bond; here it's a non-static StatDelta).
    if matches!(c.selector, Selector::BondedPair) {
        return matches!(c.effect, Effect::StatDelta { .. });
    }
    // Count-based Anima/Draw: per adjacent allied pair (Harvest Together) / per spirit lost
    // this match (What Remains) / per ally dissolved this turn (Remember Them). Owner-scoped.
    if matches!(
        c.effect,
        Effect::AnimaPerAdjacentAlliedPair { .. }
            | Effect::AnimaPerBanishedAlly { .. }
            | Effect::DrawPerBanishedThisTurn { .. }
    ) {
        return c.selector == Selector::Owner;
    }
    // War Roar: a tribal StatDelta over your imprint-sharing allies (the executor
    // resolves AlliesWithImprint with the catalog).
    if matches!(c.selector, Selector::AlliesWithImprint { .. }) {
        return matches!(c.effect, Effect::StatDelta { .. })
            && matches!(
                c.duration,
                Duration::Instant | Duration::ThisRound | Duration::Permanent
            );
    }
    // Eruption: damage to enemies adjacent to your Resonance-sharing allies (the
    // executor resolves EnemiesAdjacentToAlliesOf with the catalog).
    if matches!(c.selector, Selector::EnemiesAdjacentToAlliesOf { .. }) {
        return matches!(c.effect, Effect::Damage { .. });
    }
    // A caller summons its Kindred.
    if matches!(c.effect, Effect::Summon { .. }) {
        return c.selector == Selector::Owner;
    }
    // Last-Light Koi: its Parting lets the owner reclaim FULL Cost — consulted by the
    // voluntary Reclaim action via the PartingReclaimsFullCost exception (behavior-tested
    // in target_choice.rs: reclaim_refunds_half_cost_and_full_for_last_light_koi).
    if matches!(
        c.effect,
        Effect::Exception(RuleException::PartingReclaimsFullCost)
    ) {
        return c.selector == Selector::Owner;
    }
    // Second Farewell: re-fire one of the owner's Parting effects (a choice over the
    // owner's standing Parting-bearers → fire_doctrine(Parting)).
    if matches!(c.effect, Effect::ReTriggerParting) {
        return c.selector == Selector::Owner;
    }
    // The no-impression removals: Release (the Solace's mercy — fading-only) and
    // Banish (the IllIntent erasure — any state). Both supported for the
    // adjacency/board-wide and choice selectors the merciful/erasing cards use.
    if matches!(c.effect, Effect::Release | Effect::Banish) {
        return matches!(
            c.selector,
            Selector::AdjacentAlliesAll
                | Selector::AdjacentEnemiesAll
                | Selector::AdjacentEnemyChoose
                | Selector::AdjacentAllyChoose
                | Selector::AllOtherSpirits
                | Selector::AlliesAll
                | Selector::EnemiesAll
        );
    }
    let scope_ok = matches!(
        c.selector,
        Selector::SelfSpirit
            | Selector::Owner
            | Selector::AlliesAll
            | Selector::EnemiesAll
            | Selector::AdjacentEnemiesAll
            | Selector::AdjacentAlliesAll
            | Selector::AllOtherSpirits
            | Selector::Engager
    );
    let effect_ok = matches!(
        c.effect,
        Effect::Draw { .. }
            | Effect::AnimaDelta { .. }
            | Effect::RestoreForm { .. }
            | Effect::Damage { .. }
            | Effect::StatDelta { .. }
    );
    let duration_ok = matches!(
        c.duration,
        Duration::Instant | Duration::Permanent | Duration::ThisRound | Duration::NextRound
    );
    scope_ok && effect_ok && duration_ok
}

/// Static (aura) clauses computed on read by the engine.
pub fn supported_static_clause(c: &Clause) -> bool {
    // A Static aura holds WHILE its source stands — the engine never reads its `duration`,
    // so `WhilePresent` and `Permanent` are equivalent here (some Solace/antagonist cards
    // author the latter). Either qualifies a standing aura.
    let static_dur_ok = matches!(c.duration, Duration::WhilePresent | Duration::Permanent);
    // Movement/push keyword grants the engine honors via `keyword_active`
    // (Mobile → move legality, Steadfast → push immunity), plus Warded routed
    // through `eff_warded` at the defender combat read-site. Arcane is not aura-
    // routed (its read-sites thread the forecast), so a bond/landmark granting it
    // is not credited here.
    let aura_keyword_ok = |c: &Clause| {
        matches!(
            &c.effect,
            Effect::GrantKeyword {
                // Mobile/Steadfast → keyword_active (move/push); Warded → eff_warded
                // (defender combat routing) — all aura-routed.
                keyword: Keyword::Mobile | Keyword::Steadfast | Keyword::Warded
            }
        )
    };
    // Targeting-only reach auras the engine widens via `targeting_reach` (Shared
    // Horizon, Stargazing, The Overlook, Skylight). A non-targeting reach aura would
    // also have to widen projection/interception, which is not routed → not credited here.
    let reach_aura_ok = |c: &Clause| {
        matches!(
            &c.effect,
            Effect::ReachDelta {
                targeting_only: true,
                ..
            }
        )
    };
    // Bonded-pair auras: `combat_stats` applies StatDelta / StatShareHigher to both
    // ends while the pair is adjacent; `keyword_active` grants Mobile/Steadfast;
    // `targeting_reach` widens reach.
    // GrantEngage bonds, both honored in full_exchange: Pack Tactics chips the
    // target first (pre_chip > 0); Conspiracy (immediate) is a reactive counter-engage.
    let grant_engage_chip_ok = |c: &Clause| matches!(&c.effect, Effect::GrantEngage { pre_chip, immediate } if *pre_chip > 0 || *immediate);
    // Grief Split (RedirectDamageToPartner) and Promise (Replace(PartnerTakesIt),
    // OncePerMatch) move a lethal blow onto the partner — honored in full_exchange
    // via `damage_redirect_to_partner`.
    let damage_redirect_ok = |c: &Clause| {
        matches!(
            &c.effect,
            Effect::RedirectDamageToPartner | Effect::Replace(Replacement::PartnerTakesIt)
        )
    };
    if matches!(c.selector, Selector::BondedPair) {
        // StatDelta / StatShareHigher are applied by `combat_stats` to both ends
        // while the pair is adjacent, regardless of duration — so the while-present
        // auras AND the reactive-flattened ThisRound buffs (Race You, Call and
        // Answer: "+10 Attack each while adjacent", a documented simplification of
        // their "when one engages" text) both qualify.
        let stat_ok = matches!(
            c.effect,
            Effect::StatDelta { .. } | Effect::StatShareHigher { .. }
        ) && matches!(c.duration, Duration::WhilePresent | Duration::ThisRound);
        // The other bond auras (keyword / reach / pre-chip / damage-redirect) hold
        // only while the bond is present.
        let aura_ok = (aura_keyword_ok(c)
            || reach_aura_ok(c)
            || grant_engage_chip_ok(c)
            || damage_redirect_ok(c)
            // Throughline bond modifiers (consulted in check_throughline): Twin Telling
            // pools the pair's Imprints; Unbreakable bridges a single 1-tile gap.
            || matches!(
                c.effect,
                Effect::Exception(RuleException::PairSharesImprints)
                    | Effect::Exception(RuleException::BondHoldsUnlessSeparated)
            ))
            && static_dur_ok;
        return stat_ok || aura_ok;
    }
    // Landmark occupant aura: the terrain's clauses buff (StatDelta/RetaliationDelta),
    // grant a movement keyword (Crossroads → Mobile), or widen reach (The Overlook,
    // Skylight) for the spirit standing on it.
    if matches!(c.selector, Selector::OccupantHere) {
        return (matches!(
            c.effect,
            Effect::StatDelta { .. } | Effect::RetaliationDelta { .. }
            // The Threshold: its occupant can't be pushed (honored in push_away
            // via occupant_restricted).
            | Effect::Restrict(Restriction::BePushed)
            // Common Ground: bonds touching its occupant cost 0 (honored at the
            // bond-attach chokepoint via tile_terrain_exception).
            | Effect::Exception(RuleException::BondsFreeOnLandmark)
            // Rainpool: a face-up landmark doubles its controller's Partings (routed
            // seat-wide via exception_active's terrain scan — a documented simplification
            // of "the occupant's Partings", since the exception model is seat-scoped).
            | Effect::Exception(RuleException::PartingTriggersTwice)
            // Shrine of the Nameless: a fading occupant fuels imprint-free evolution
            // (consulted positionally in legal_evolutions via shrine_fading_fuel).
            | Effect::Exception(RuleException::FadingFuelIgnoresImprint)
        ) || aura_keyword_ok(c)
            || reach_aura_ok(c))
            && static_dur_ok;
    }
    // The Trellis: the landmark also buffs allies ADJACENT to it (the "adjacent"
    // half of `OccupantAndAdjacentAllies`, wired in `combat_stats`). StatDelta only.
    if matches!(c.selector, Selector::OccupantAndAdjacentAllies) {
        return matches!(c.effect, Effect::StatDelta { .. }) && static_dur_ok;
    }
    // AdjacentEnemiesAll StatDelta debuffs: applied by the spirit-aura loop (a
    // spirit standing aura) AND the Co-Conspirators bond path. StatDelta — the
    // engine does not compute RetaliationDelta for non-self adjacents. TraitSilence
    // (The Smudge / Null Choir) blanks an adjacent enemy's trait-borne combat value
    // (the Chorus bonus), honored in combat_stats via `trait_silenced`.
    if matches!(c.selector, Selector::AdjacentEnemiesAll) {
        return matches!(c.effect, Effect::StatDelta { .. } | Effect::TraitSilence)
            && static_dur_ok;
    }
    let scope_ok = matches!(
        c.selector,
        Selector::SelfSpirit
            | Selector::AdjacentAlliesAll
            | Selector::AlliesAll
            | Selector::AlliesInReach
            | Selector::AlliesWithImprint { .. }
            | Selector::EnemiesWithinTiles { .. }
            | Selector::EnemiesEngagingAdjacentAllies
            | Selector::EnemiesAll
    );
    let effect_ok = matches!(
        c.effect,
        Effect::StatDelta { .. } | Effect::RetaliationDelta { .. } | Effect::GrantSharedResonance
            | Effect::CounterAura { .. } | Effect::DamageReduction { .. } | Effect::EdgeNegate
            | Effect::Replace(Replacement::SurviveBanishAt { .. })
            // Keyword grants are intrinsic catalog data the engine honors.
            | Effect::GrantKeyword { keyword: Keyword::Lurk }
            // Momentum auras and cost auras are derived/honored on read.
            | Effect::MomentumMod { .. } | Effect::CostDelta { .. }
            // Routed rule exceptions (consulted at their rule's chokepoint).
            | Effect::Exception(RuleException::GlimpseLooksOneMore)
            | Effect::Exception(RuleException::BondsCostZero)
            // Errata: "counts as every Imprint" — a Throughline wildcard (check_throughline)
            // and frees the owner's evolutions from the shared-Imprint rule (legal_evolutions).
            | Effect::Exception(RuleException::AllImprintsShared)
            // The Vigilkeeper: allies' Partings fire twice (consulted in fire_doctrine
            // via exception_active; behavior-tested in rule_exceptions.rs).
            | Effect::Exception(RuleException::PartingTriggersTwice)
            // Targeting-only reach auras the engine widens via `targeting_reach`
            // (Pathfinder Ibex: AdjacentAlliesAll reach +1 forward).
            | Effect::ReachDelta { targeting_only: true, .. }
            // Beacon: a face-up Landmark reveals adjacent enemy Fabrications to its owner
            // (consulted by the view's redaction via beacon_reveals_fab).
            | Effect::RevealFabrication
            // Queen of the Quiet Garden: amplifies the owner's Throughline buff
            // (summed in throughline_grant at completion).
            | Effect::ThroughlineGrant { .. }
            // Matron of the Long Goodbye: spirits the owner evolves arrive +10/+10
            // (consulted at the evolve arrival via exception_active).
            | Effect::Exception(RuleException::EvolveArrivesBuffed)
            // Otterling Magus: the owner's Rituals affect one extra target (armed at
            // CastRitual, applied at the Choose).
            | Effect::Exception(RuleException::RitualsExtraTarget)
            // Duet Ascendant: a flat bond stat grant the owner's bonded spirits receive
            // (summed in owner_grants_bond_stat at combat_stats).
            | Effect::BondStatGrant { .. }
            // Badgermarshal: adjacent allies take less from enemy Momentum chains
            // (honored in full_exchange via chain_damage_reduction).
            | Effect::ChainDamageReductionAura { .. }
            // Grudge-Kept: +Attack per enemy impression (honored in combat_stats).
            | Effect::AttackPerEnemyImpression { .. }
            // The Worst Version: Attack copies the highest enemy (honored in combat_stats).
            | Effect::CopyHighestEnemyAttack
            // The Long Rest: adjacent dissolves leave no impression (honored at dissolution).
            | Effect::Exception(RuleException::NoImpressionOnAdjacentDissolve)
            // The Closing Book: immune to interception (honored in interception).
            | Effect::Exception(RuleException::ImmuneToInterception)
            // The Forgiven Debt: unbanishable by an Echo attacker (honored in full_exchange + forecast).
            | Effect::Exception(RuleException::UnbanishableByEcho)
            // Ferrier of the Salt Road: Fade reclaim regains +1 (honored in the Reclaim handler).
            | Effect::Exception(RuleException::FadeReclaimsExtraAnima)
            // Hush: an adjacent spirit's Parting is suppressed (honored in fire_doctrine).
            | Effect::Exception(RuleException::SuppressesAdjacentParting)
            // The Unforgiving: its arcane strikes ignore Warded (honored in full_exchange + interception).
            | Effect::Exception(RuleException::StrikesIgnoreWarded)
            // The Lullaby: adjacent enemies lose Echo eligibility (honored in full_exchange +
            // interception via echo_suppressed).
            | Effect::Exception(RuleException::SuppressesAdjacentEnemyEcho)
            // Evolution forms (engine already honors these — pure under-count credits):
            // Whisperer at the Door: enemies engaging this spirit are −Attack (SelfSpirit/
            // EngagerAttackDelta, read in combat_stats at the attacker's stat fold).
            | Effect::EngagerAttackDelta { .. }
            // Herald of the Ill Intent: an enemy pushed onto an impression-bearing tile takes
            // damage (Owner/ImpressionPushDamage, honored in push_away).
            | Effect::ImpressionPushDamage { .. }
            // Arbiter Imperishable: a standing aura makes the owner's spirits unpushable
            // (AlliesAll/Restrict(BePushed), honored in push_away via owner_push_immune).
            | Effect::Restrict(Restriction::BePushed)
            // What's-Its-Name: unnameable — no Ritual/free-target effect may aim at it
            // (SelfSpirit, honored at the target-eligibility chokepoint via ritual_untargetable).
            | Effect::Restrict(Restriction::BeTargetedByRituals)
            // Lacuna: the tiles adjacent to it cannot gain new impressions (SelfSpirit,
            // honored at the impression-laying chokepoint via lacuna_denies_impression).
            | Effect::Restrict(Restriction::GainImpressions)
            // Erasure's Patience: impressions on tiles adjacent to it do not score
            // (honored at the Nightfall scoring chokepoint via erasure_patience_cooled_tiles).
            | Effect::Exception(RuleException::AdjacentImpressionsDontScore)
            // A genuine no-op aura — the keyword/flavor IS the card (Foundling dispositions,
            // keyword-only tokens, The Gnawing). Nothing to fire, correctly.
            | Effect::NoEffect
    );
    // Oathkeeper Adamant: an AdjacentAlliesAll aura GRANTING Mobile/Steadfast is routed by
    // `granted_keyword` (the move/push read-sites are aura-aware). Warded/Arcane grants from a
    // spirit aura are NOT routed, so they are not credited here.
    let adjacent_grant_ok = matches!(c.selector, Selector::AdjacentAlliesAll)
        && matches!(
            &c.effect,
            Effect::GrantKeyword {
                keyword: Keyword::Mobile | Keyword::Steadfast
            }
        );
    // Wolverine Wearing a Trap: a SelfSpirit GrantKeyword restates a keyword the card carries
    // intrinsically (Steadfast) — `keyword_active` honors the intrinsic keyword, so the clause
    // is engine-backed. (Mobile/Steadfast/Warded are the aura-routed keywords.)
    let self_grant_ok = matches!(c.selector, Selector::SelfSpirit)
        && matches!(
            &c.effect,
            Effect::GrantKeyword {
                keyword: Keyword::Mobile | Keyword::Steadfast | Keyword::Warded
            }
        );
    let scope_ok = scope_ok || matches!(c.selector, Selector::Owner | Selector::Survivor);
    (scope_ok && effect_ok || adjacent_grant_ok || self_grant_ok) && static_dur_ok
}

pub fn supported_condition(c: &Condition) -> bool {
    matches!(
        c,
        Condition::Always
            | Condition::WhileDamaged
            | Condition::WhileUndamaged
            | Condition::WhileAdjacentToAlly
            | Condition::WhileFaceDown
            | Condition::AdjacentAlliesShareResonance { .. }
            | Condition::OncePerMatch
            | Condition::PairAdjacent
            | Condition::YouControlABond
            | Condition::CostAtMost { .. }
            | Condition::PayForm { .. }
    )
}

pub fn supported_trigger(t: Trigger) -> bool {
    matches!(
        t,
        Trigger::OnReveal
            | Trigger::OnPlay
            | Trigger::Parting
            | Trigger::OnAnyBanish
            | Trigger::OnDefeat
            | Trigger::AtFlow
            | Trigger::Static
            | Trigger::OnAllyFading
            | Trigger::OnEngageResolved
            | Trigger::OnYouPlayBond
            // The Unfiled: "When Attuned" fires at the owner's Flow while attuned
            // (fire_at_flow scans standing OnAttuned spirits).
            | Trigger::OnAttuned
            // Vale Eternal: fires when one of the owner's Throughlines completes.
            | Trigger::OnThroughlineComplete
            // Magistrate of Masks / Warden-Breaker / Elegist Wren: event self-buffs.
            | Trigger::OnFabricationRevealed
            | Trigger::OnDefeatWarded
            | Trigger::OnAllyPartsInReach
            // The Devouring Margin — OnMove fires when a Solace eater shifts (eat the
            // impression it lands on + heal), via the normal move path.
            | Trigger::OnMove
            // An Unwritten Unwrites (dissolves) — Footnote / Sentence Fragment mill a deck's
            // top card. Fired in `flow.rs` (the fade step) via the generic `fire_doctrine` path.
            | Trigger::OnUnwrite
            // A Foundling is befriended — Pigeon Carrying a Message Never Delivered draws.
            // Fired in `strays.rs` (`befriend`) via the generic `fire_doctrine` path.
            | Trigger::OnBefriend
    )
}

/// The ratchet's definition: every spec's trigger, condition, and clauses
/// land in an executed tranche.
pub fn card_fully_supported(name: &str) -> bool {
    match canon_effects().specs.get(crate::cards::key_of(name)) {
        None => false,
        Some(specs) => specs.iter().all(|s| {
            if !supported_trigger(s.trigger) || !supported_condition(&s.condition) {
                return false;
            }
            if s.trigger == Trigger::Static {
                s.clauses.iter().all(supported_static_clause)
            } else if s.trigger == Trigger::AtFlow {
                // Persistent-source Flow effects fired by the engine at start of
                // turn (Wellspring, Hearth, Shared Umbrella). `OncePerMatch` marks a
                // DELAYED played-card grant (Patience) — now scheduled at play time
                // (`schedule_flow_anima`) and paid at the owner's next Flow.
                matches!(
                    s.condition,
                    Condition::Always | Condition::PairAdjacent | Condition::OncePerMatch
                ) && s.clauses.iter().all(supported_atflow_clause)
            } else {
                // Bond triggers (OnDefeat) inherently require the pair adjacent — allow
                // PairAdjacent there (Common Cause). A played card may also gate on
                // controlling a Bond (Gather In). Everything else is unconditional.
                let cond_ok = s.condition == Condition::Always
                    || (s.trigger == Trigger::OnDefeat && s.condition == Condition::PairAdjacent)
                    || s.condition == Condition::YouControlABond
                    // The Fog of Elsewhere: a played card gating its target by Cost.
                    || matches!(s.condition, Condition::CostAtMost { .. })
                    // Ragewoken Bison: a "may pay HP" gate.
                    || matches!(s.condition, Condition::PayForm { .. })
                    // Tooth in the Margin: an OnPlay OncePerMatch buff fires on arrival
                    // (the single play IS the once-per-match instance).
                    || (s.trigger == Trigger::OnPlay && s.condition == Condition::OncePerMatch);
                // A reveal that "becomes a Landmark" (The Long Table) carries an
                // ongoing WhilePresent aura, not an instant — credit it through the
                // static-aura predicate (combat_stats reads it once revealed).
                cond_ok
                    && s.clauses.iter().all(|c| {
                        supported_instant_clause(c)
                            || (c.duration == Duration::WhilePresent && supported_static_clause(c))
                    })
            }
        }),
    }
}

/// AtFlow clauses fired by `fire_at_flow` for the active seat's persistent
/// sources: a Landmark's anima tithe (Wellspring) or occupant heal (Hearth),
/// a Bond's pair heal (Shared Umbrella), or — evolution forms, already wired
/// in `fire_at_flow` — a standing spirit's heal-adjacent (Elder of the Unbroken
/// Watch: AdjacentAlliesAll/RestoreForm) or Flow Glimpse (Sage of And-Then:
/// Owner/PeekDeck, the owner is active during its own Flow so the choice is safe).
/// A spirit's OWN-state Flow effects are resolved deterministically too:
/// SelfSpirit/RestoreForm (Quiet Tide), SelfSpirit/GrowEachFlow (the Foal grows,
/// capped), and Owner/ImpressionRemoveTarget (The Last Warm Page erodes one
/// adjacent enemy mark).
pub fn supported_atflow_clause(c: &Clause) -> bool {
    matches!(
        (&c.selector, &c.effect),
        (Selector::Owner, Effect::AnimaDelta { .. })
            | (Selector::OccupantHere, Effect::RestoreForm { .. })
            | (Selector::BondedPair, Effect::RestoreForm { .. })
            | (Selector::AdjacentAlliesAll, Effect::RestoreForm { .. })
            | (Selector::SelfSpirit, Effect::RestoreForm { .. })
            | (Selector::SelfSpirit, Effect::GrowEachFlow { .. })
            | (Selector::Owner, Effect::PeekDeck { .. })
            | (Selector::Owner, Effect::ImpressionRemoveTarget)
    )
}
