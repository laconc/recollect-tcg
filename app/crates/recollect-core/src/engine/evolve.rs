//! The `evolve` half of the family: applying each `Event` to `GameState`.
//! A trait impl, so it needs no re-export — `mod evolve;` makes it take effect.
use super::*;

impl GameState {
    /// Lay the banisher's mark on a tile, faction-aware: the **Solace** leaves no board impression —
    /// it tallies its erasure (banked, off-board) — while every other faction stamps its impression
    /// (one per tile, overwriting the old). The single home for "what the Solace removes leaves
    /// nothing, but still scores."
    pub(crate) fn lay_mark(&mut self, tile: usize, by: Seat) {
        if self.rules.factions[by as usize] == crate::types::Faction::Solace {
            self.board[tile].impressions.clear();
            self.solace_erasures = self.solace_erasures.saturating_add(1);
        } else {
            self.board[tile].impressions = vec![by];
        }
    }
}

impl AggregateRules for GameState {
    type Phase = GameLifecycle;
    type Command = Command;
    type Event = Event;
    type Error = Reject;
    type Ctx = TurnCtx;

    /// The coarse lifecycle, computed directly from state — recollect never
    /// drives the machine's transition; `decide` owns the real lifecycle.
    fn phase(&self) -> GameLifecycle {
        if matches!(self.phase, Phase::Finished { .. }) {
            GameLifecycle::Over
        } else {
            GameLifecycle::Live
        }
    }

    fn decide(&self, cmd: &Command, ctx: &mut TurnCtx) -> Result<Vec<Event>, Reject> {
        decide_impl(self, cmd, ctx)
    }

    fn evolve(&mut self, ev: &Event) {
        // Resource events (anima/draw/hand/glimpse) target the ACTIVE
        // SLOT's player in 2v2 — A1 and A2 share team Seat but not hands.
        // These events only ever fire on the acting player's own turn, so the
        // active slot is the correct player whenever its team matches `seat`.
        let slot_for = |st: &GameState, seat: Seat| -> crate::types::SeatSlot {
            if st.is_2v2() && st.active_slot.team() == seat {
                st.active_slot
            } else if seat == Seat::A {
                crate::types::SeatSlot::A1
            } else {
                crate::types::SeatSlot::B1
            }
        };
        match ev {
            Event::MatchStarted => {}
            Event::AnimaGained { seat, amount, .. } => {
                let s = slot_for(self, *seat);
                let cur = self.player_slot(s).anima;
                self.player_slot_mut(s).anima = cur.saturating_add(*amount);
            }
            Event::AnimaSpent { seat, amount } => {
                let s = slot_for(self, *seat);
                let cur = self.player_slot(s).anima;
                self.player_slot_mut(s).anima = cur.saturating_sub(*amount);
            }
            Event::CardDrawn { seat } => {
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if !p.deck.is_empty() {
                    let c = p.deck.remove(0);
                    p.hand.push(c);
                }
                p.peeked_top = None;
            }
            Event::ReleaseRequired { seat } => {
                if matches!(self.phase, Phase::Acting) {
                    self.phase = Phase::PendingRelease { seat: *seat };
                }
            }
            Event::CardReleased { seat, hand_index } => {
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if (*hand_index as usize) < p.hand.len() {
                    let c = p.hand.remove(*hand_index as usize);
                    p.deck.push(c); // the bottom of the page
                }
                if matches!(self.phase, Phase::PendingRelease { .. }) {
                    self.phase = Phase::Acting;
                }
            }
            Event::Glimpsed { seat, peeked: _ } => {
                // Glimpse (§5) opened: spend the once-per-turn flag. The BURN cost
                // and the keep-or-bottom choice ride behind this event (the
                // `GlimpseBurn` then `Glimpse` pending choices); `peeked_top` is set
                // only when a Glimpse resolves KEEP (below). While the choices are
                // pending the owner sees them through its own `pending` view, never
                // the opponent.
                let s = slot_for(self, *seat);
                self.player_slot_mut(s).glimpsed_this_turn = true;
            }
            Event::GlimpseBurned { seat, hand_index } => {
                // Glimpse (§5) BURN cost: the chosen hand card leaves play entirely.
                // The `GlimpseBurn` pending choice is consumed; the keep-or-bottom
                // `Glimpse` opens via the `ChoiceOffered` that follows this event.
                if matches!(
                    self.pending_choice,
                    Some(crate::state::PendingChoice::GlimpseBurn { .. })
                ) {
                    self.pending_choice = None;
                }
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if (*hand_index as usize) < p.hand.len() {
                    p.hand.remove(*hand_index as usize);
                }
            }
            Event::GlimpseResolved { seat, kept } => {
                // The card under glimpse rides the pending choice; take it as we settle.
                if let Some(crate::state::PendingChoice::Glimpse { top, .. }) =
                    self.pending_choice.take()
                {
                    let s = slot_for(self, *seat);
                    let p = self.player_slot_mut(s);
                    if *kept {
                        // Stays on top — and now the owner knows it (the +1 Anima is
                        // foregone; the resolution event carries no anima).
                        p.peeked_top = Some(top);
                    } else {
                        // Bottomed for focus: the top card goes to the bottom of the
                        // page (the +1 Anima rides a separate `AnimaGained`). The top
                        // is now unknown, so the prior peek no longer holds.
                        if !p.deck.is_empty() {
                            p.deck.remove(0);
                        }
                        p.deck.push(top);
                        p.peeked_top = None;
                    }
                }
                self.resume_after_choice();
            }
            Event::SpiritPlayed {
                seat,
                card,
                tile,
                attack,
                defense,
                hp,
                face_down,
            } => {
                let s = slot_for(self, *seat);
                let placed = if self.is_2v2() { Some(s) } else { None };
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == card) {
                    p.hand.remove(i);
                }
                self.moved_this_turn.push(*tile); // A fresh arrival is summoning-sick — no Move this turn
                self.board[*tile as usize].spirit = Some(Spirit {
                    replacement_used: false,
                    holding: false,
                    face_down: *face_down,
                    is_token: false,
                    placed_by: placed,
                    card: *card,
                    owner: *seat,
                    attack: *attack,
                    defense: *defense,
                    hp: *hp,
                    hp_max: *hp,
                    fading: false,
                    banished_by: None,
                    intercepted_this_round: false,
                    traits_stripped: false,
                    traits_stripped_until: None,
                    kw_grants: Vec::new(),
                    no_engage_until: 0,
                    throughline_done: false,
                    copied_reach: None,
                    fade_deadline: None,
                });
                if *seat == Seat::A {
                    self.player_a.first_placement_done = true;
                }
                // The Half-Remembered copies the last FACE-UP spirit you played (a lurker is
                // not yet a memory — it hasn't been spoken aloud).
                if !*face_down {
                    self.last_played_spirit[*seat as usize] = Some(*card);
                }
            }
            Event::SpiritCopiedStats {
                tile,
                attack,
                defense,
                hp_max,
            } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.attack = *attack;
                    sp.defense = *defense;
                    sp.hp_max = *hp_max;
                    sp.hp = *hp_max;
                }
            }
            Event::ReachCopied { tile, reach } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.copied_reach = Some(*reach);
                }
            }
            Event::Struck {
                from_tile,
                to_tile,
                damage,
                kind,
                ..
            } => {
                if let Some(sp) = self.board[*to_tile as usize].spirit.as_mut() {
                    sp.hp -= damage;
                }
                if matches!(kind, StrikeKind::Interception)
                    && let Some(sp) = self.board[*from_tile as usize].spirit.as_mut()
                {
                    sp.intercepted_this_round = true;
                }
            }
            Event::ChoiceOffered { choice } => {
                let seat = match choice {
                    crate::state::PendingChoice::Peek { seat, .. }
                    | crate::state::PendingChoice::GlimpseBurn { seat, .. }
                    | crate::state::PendingChoice::Glimpse { seat, .. }
                    | crate::state::PendingChoice::Target { seat, .. }
                    | crate::state::PendingChoice::Recover { seat, .. } => *seat,
                };
                // A play that fires several choice clauses (Dig In, The Long Watch)
                // queues the extras behind the one in flight; they surface FIFO as
                // each resolves (see `resume_after_choice`).
                if self.pending_choice.is_some() {
                    self.choice_queue.push(choice.clone());
                } else {
                    self.pending_choice = Some(choice.clone());
                    self.phase = Phase::PendingChoice { seat };
                }
            }
            Event::PeekTaken { seat, index } => {
                if let Some(crate::state::PendingChoice::Peek { looked, .. }) =
                    self.pending_choice.take()
                {
                    let p = self.player_mut(*seat);
                    for _ in 0..looked.len() {
                        p.deck.pop(); // the glimpsed cards leave the top
                    }
                    for (i, c) in looked.into_iter().enumerate() {
                        if i == *index as usize {
                            p.hand.push(c);
                        } else {
                            p.deck.insert(0, c);
                        }
                    }
                }
                self.resume_after_choice();
            }
            Event::TargetChosen { .. } => {
                self.pending_choice = None;
                self.resume_after_choice();
            }
            Event::SpiritPushed { from, to } => {
                let sp = self.board[*from as usize].spirit.take();
                self.board[*to as usize].spirit = sp;
            }
            Event::RitualCast { seat, card } => {
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == card) {
                    p.hand.remove(i);
                }
            }
            Event::BondAttached {
                seat,
                card,
                tile_a,
                tile_b,
            } => {
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == card) {
                    p.hand.remove(i);
                }
                self.bonds.push(crate::state::Bond {
                    card: *card,
                    owner: *seat,
                    tile_a: *tile_a,
                    tile_b: *tile_b,
                });
            }
            Event::BondBroken { card } => {
                self.bonds.retain(|b| b.card != *card);
            }
            Event::LandmarkPlaced { seat, card, tile } => {
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == card) {
                    p.hand.remove(i);
                }
                self.board[*tile as usize].terrain = Some(crate::state::Terrain {
                    card: *card,
                    owner: *seat,
                    kind: crate::state::TerrainKind::Landmark,
                    face_down: false,
                });
            }
            Event::FabricationSet { seat, card, tile } => {
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == card) {
                    p.hand.remove(i);
                }
                self.board[*tile as usize].terrain = Some(crate::state::Terrain {
                    card: *card,
                    owner: *seat,
                    kind: crate::state::TerrainKind::Fabrication,
                    face_down: true,
                });
            }
            Event::FabricationRevealed { tile } => {
                if let Some(t) = self.board[*tile as usize].terrain.as_mut() {
                    t.face_down = false;
                }
            }
            Event::FabricationSpent { tile } => {
                self.board[*tile as usize].terrain = None;
            }
            Event::UnwrittenManifested {
                card,
                tile,
                attack,
                defense,
                hp,
            } => {
                // An Unwritten is the Solace's (seat B) creature — no impression ever.
                self.moved_this_turn.push(*tile); // A fresh arrival is summoning-sick
                self.board[*tile as usize].spirit = Some(Spirit {
                    replacement_used: false,
                    holding: false,
                    face_down: false,
                    is_token: true,
                    placed_by: None,
                    card: *card,
                    owner: Seat::B,
                    attack: *attack,
                    defense: *defense,
                    hp: *hp,
                    hp_max: *hp,
                    fading: false,
                    banished_by: None,
                    intercepted_this_round: false,
                    traits_stripped: false,
                    traits_stripped_until: None,
                    kw_grants: Vec::new(),
                    no_engage_until: 0,
                    throughline_done: false,
                    copied_reach: None,
                    fade_deadline: None,
                });
            }
            Event::ImpressionUnwritten { tile } => {
                self.board[*tile as usize].impressions.clear(); // nothing remains
            }
            Event::ImpressionForgotten { tile } => {
                // The Solace unwrites the player's memory: the tile is left empty (the Unwritten
                // leave no mark) and the erasure tally goes +1 — that is how forgetting scores.
                self.board[*tile as usize].impressions.clear();
                self.solace_erasures = self.solace_erasures.saturating_add(1);
            }
            Event::SpiritReleased { tile } => {
                // Released: the spirit leaves, gently, and nothing is left — no
                // body, no impression. The merciful counterpart to a banishment.
                self.board[*tile as usize].spirit = None;
            }
            Event::SpiritBounced { tile } => {
                // Un-played: returned to its owner's hand (no body, no impression, no
                // banish triggers). A token has no card to return — it just ceases.
                if let Some(sp) = self.board[*tile as usize].spirit.take()
                    && !sp.is_token
                {
                    self.player_mut(sp.owner).hand.push(sp.card);
                }
            }
            Event::UnwrittenShifted {
                from,
                to,
                eats_impression,
            } => {
                let sp = self.board[*from as usize].spirit.take();
                // Page-Eater: when this Unwritten arrives on an impression-bearing tile, it eats the
                // player's memory — the tile is left empty (no Solace mark) and the erasure tally
                // goes +1. (`eats_impression` is computed by the inward shift for movers carrying
                // `ImpressionEat`; other Unwritten leave the tile alone.)
                if *eats_impression {
                    self.board[*to as usize].impressions.clear();
                    self.solace_erasures = self.solace_erasures.saturating_add(1);
                }
                self.board[*to as usize].spirit = sp;
            }
            Event::UnwritingTold { seat, card } => {
                self.unwriting_told_this_round = true;
                // The Unwriting event is a one-shot: discard it from the Solace's hand (like a Ritual).
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == card) {
                    p.hand.remove(i);
                }
            }
            Event::DeckMilled { seat } => {
                let p = self.player_mut(*seat);
                if !p.deck.is_empty() {
                    p.deck.remove(0); // the top (next to draw) is unwritten
                }
            }
            Event::ImpressionsStoppedScoring { round } => {
                self.impressions_dormant_round = Some(*round);
            }
            Event::LandmarkSilenced { tile, round } => {
                self.silenced_terrain = Some((*tile, *round));
            }
            Event::TilesGoneCalm { tiles, round } => {
                for &t in tiles {
                    self.calm_tiles.push((t, *round));
                }
            }
            Event::StrayTelegraphed {
                tile,
                surface_round,
                midnight,
            } => {
                self.stray_telegraph = Some(crate::state::StrayTelegraph {
                    tile: *tile,
                    surface_round: *surface_round,
                    midnight: *midnight,
                });
            }
            Event::StrayTelegraphCleared => {
                self.stray_telegraph = None;
            }
            Event::StraySurfaced {
                card,
                tile,
                temperament,
                veiled,
                hp,
            } => {
                self.stray = Some(crate::state::Stray {
                    card: *card,
                    tile: *tile,
                    temperament: *temperament,
                    veiled: *veiled,
                    courtship: 0,
                    courted_by: None,
                    hp: *hp,
                    hp_max: *hp,
                });
                self.stray_telegraph = None;
            }
            Event::StrayStruck { hp, .. } => {
                if let Some(s) = self.stray.as_mut() {
                    s.hp = *hp;
                }
            }
            Event::StrayUnveiled => {
                if let Some(s) = self.stray.as_mut() {
                    s.veiled = false;
                }
            }
            Event::StrayCourted { seat, courtship } => {
                if let Some(s) = self.stray.as_mut() {
                    s.courtship = *courtship;
                    s.courted_by = Some(*seat);
                }
            }
            Event::StrayBefriended {
                seat,
                card,
                tile,
                attack,
                defense,
                hp,
            } => {
                // Becomes a normal owned spirit under Held Ground law.
                self.board[*tile as usize].spirit = Some(Spirit {
                    replacement_used: false,
                    holding: false,
                    face_down: false,
                    is_token: false,
                    placed_by: None,
                    card: *card,
                    owner: *seat,
                    attack: *attack,
                    defense: *defense,
                    hp: *hp,
                    hp_max: *hp,
                    fading: false,
                    banished_by: None,
                    intercepted_this_round: false,
                    traits_stripped: false,
                    traits_stripped_until: None,
                    kw_grants: Vec::new(),
                    no_engage_until: 0,
                    throughline_done: false,
                    copied_reach: None,
                    fade_deadline: None,
                });
                self.stray = None;
            }
            Event::StrayBanished { tile, impression } => {
                self.lay_mark(*tile as usize, *impression);
                self.stray = None;
            }
            Event::StrayDenied { tile: _ } => {
                // A hidden Stray is denied entry by an Overwrite: it simply leaves — no
                // impression, no reveal, no identity surfaced. Just empty the slot. (The
                // overwriter lands via the OverwroteStray { success } that follows.)
                self.stray = None;
            }
            Event::OverwroteStray {
                seat,
                card,
                tile,
                success,
                damage_to_stray,
                attack,
                defense,
                attacker_hp_left,
                attacker_hp_max,
            } => {
                // The form was a card in hand — consume the played copy (the overwriter is
                // played from hand exactly like a spirit Overwrite).
                {
                    let s = slot_for(self, *seat);
                    let p = self.player_slot_mut(s);
                    if let Some(i) = p.hand.iter().position(|c| c == card) {
                        p.hand.remove(i);
                    }
                }
                let idx = *tile as usize;
                if *success {
                    // The wild fell: lay the overwriter's banisher-impression beneath
                    // (faction-aware — the Solace tallies instead) and empty the Stray slot,
                    // then the overwriter takes the cleared tile. A spirit never coexists with
                    // a Stray, so clearing `self.stray` here is what keeps the board legal.
                    self.lay_mark(idx, *seat);
                    self.stray = None;
                    self.moved_this_turn.push(*tile); // the overwriter arrived — summoning-sick
                    self.board[idx].spirit = Some(Spirit {
                        replacement_used: false,
                        holding: false,
                        face_down: false,
                        is_token: false,
                        placed_by: if self.is_2v2() {
                            Some(self.active_slot)
                        } else {
                            None
                        },
                        card: *card,
                        owner: *seat,
                        attack: *attack,
                        defense: *defense,
                        hp: *attacker_hp_left,
                        hp_max: *attacker_hp_max,
                        fading: *attacker_hp_left <= 0,
                        banished_by: if *attacker_hp_left <= 0 {
                            Some(seat.other())
                        } else {
                            None
                        },
                        intercepted_this_round: false,
                        traits_stripped: false,
                        traits_stripped_until: None,
                        kw_grants: Vec::new(),
                        no_engage_until: 0,
                        throughline_done: false,
                        copied_reach: None,
                        // An overwriter that arrives spent (≤0 HP) was banished in the
                        // exchange — it stands Faded into its owner's next turn-end (this is
                        // the owner's own turn ⇒ next round). A survivor never fades.
                        fade_deadline: if *attacker_hp_left <= 0 {
                            Some(fade_deadline_round(self.round, self.active, *seat))
                        } else {
                            None
                        },
                    });
                } else {
                    // The wild survived: the overwriter dissolved (no impression, no body);
                    // the damage it dealt persists on the Stray in its slot.
                    if let Some(s) = self.stray.as_mut() {
                        s.hp -= *damage_to_stray;
                    }
                }
                if *seat == Seat::A {
                    self.player_a.first_placement_done = true;
                }
            }
            Event::SpiritEvolved {
                seat,
                tile,
                to,
                attack,
                defense,
                hp,
                keeps_throughline,
                ..
            } => {
                // The form was a card in hand — consume the played copy.
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == to) {
                    p.hand.remove(i);
                }
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.card = *to;
                    sp.attack = *attack;
                    sp.defense = *defense;
                    sp.hp = *hp; // full-HP arrival — Lever 1, eternal
                    sp.hp_max = *hp;
                    sp.fading = false; // evolution clears Fading
                    sp.banished_by = None;
                    sp.fade_deadline = None; // evolving redeems the standing-Faded base
                    // The Throughline buff is once-per-body (§5.4). A FABLED form is a
                    // continuation of a healthy base — it KEEPS the base's `throughline_done`
                    // (so a completed base arrives already done, locked). A PRIMAL is a fresh
                    // becoming off a Fading base — it does NOT inherit; it arrives re-completable.
                    // `decide` carries the tier decision in `keeps_throughline`.
                    if !keeps_throughline {
                        sp.throughline_done = false;
                    }
                }
            }
            Event::SpiritDevolved {
                seat,
                tile,
                to,
                attack,
                defense,
                hp,
                ..
            } => {
                // Devolution (§5): the played BASE card (`to`) recedes the standing-Faded
                // form a tier down. Consume the base from the seat's hand (it was the
                // played card), then replace the form with the base at FULL HP, fade
                // cleared (rescued), and SUMMONING-SICK — `moved_this_turn` blocks its
                // move/evolve until the owner's next turn (the turn-end clears the flag).
                let s = slot_for(self, *seat);
                let p = self.player_slot_mut(s);
                if let Some(i) = p.hand.iter().position(|c| c == to) {
                    p.hand.remove(i);
                }
                self.moved_this_turn.push(*tile); // the rescued base is summoning-sick
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.card = *to;
                    sp.attack = *attack;
                    sp.defense = *defense;
                    sp.hp = *hp; // full HP — rescued
                    sp.hp_max = *hp;
                    sp.fading = false; // the fade is cleared
                    sp.banished_by = None;
                    sp.fade_deadline = None; // out of the standing-Faded window — saved
                    sp.throughline_done = false; // a fresh base earns its Throughline anew
                }
            }
            Event::SpiritManifested {
                seat,
                card,
                tile,
                attack,
                defense,
                hp,
            } => {
                self.moved_this_turn.push(*tile); // A fresh arrival is summoning-sick
                self.board[*tile as usize].spirit = Some(Spirit {
                    replacement_used: false,
                    holding: false,
                    face_down: false,
                    is_token: true,
                    placed_by: None,
                    card: *card,
                    owner: *seat,
                    attack: *attack,
                    defense: *defense,
                    hp: *hp,
                    hp_max: *hp,
                    fading: false,
                    banished_by: None,
                    intercepted_this_round: false,
                    traits_stripped: false,
                    traits_stripped_until: None,
                    kw_grants: Vec::new(),
                    no_engage_until: 0,
                    throughline_done: false,
                    copied_reach: None,
                    fade_deadline: None,
                });
            }
            Event::TokenDissolved { tile } => {
                self.board[*tile as usize].spirit = None; // no impression — irreplaceable
            }
            Event::SpiritRevealed { tile } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.face_down = false;
                }
            }
            Event::OrdersSet { tile, hold } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.holding = *hold;
                }
            }
            Event::EffectTempStat {
                tile,
                attack,
                defense,
                until_round,
            } => {
                self.temp_mods.push(crate::state::TempMod {
                    tile: *tile,
                    attack: *attack,
                    defense: *defense,
                    until_round: *until_round,
                });
            }
            Event::ReachBuffed {
                seat,
                forward,
                all_directions,
                until_round,
                targeting_only,
                tile,
            } => {
                self.temp_reach.push(crate::state::TempReach {
                    seat: *seat,
                    forward: *forward,
                    all_directions: *all_directions,
                    until_round: *until_round,
                    targeting_only: *targeting_only,
                    tile: *tile,
                });
            }
            Event::Restricted {
                seat,
                restriction,
                until_round,
            } => {
                self.temp_restrict.push(crate::state::TempRestrict {
                    seat: *seat,
                    restriction: *restriction,
                    until_round: *until_round,
                });
            }
            Event::RetaliationBuffed {
                tile,
                delta,
                until_round,
            } => {
                self.temp_retaliation.push((*tile, *delta, *until_round));
            }
            Event::ReplacementSurvived { tile, form } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.hp = *form;
                    sp.replacement_used = true;
                }
            }
            Event::EffectRestored { tile, amount } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.hp = (sp.hp + amount).min(sp.hp_max);
                    // DECIDED (design): restore is a PURE HEAL — it raises HP
                    // only and never clears `fading`. A dissolving spirit cannot
                    // be brought back by a heal. Targeting is fading-inclusive
                    // (see restore_targets) so the heal still LANDS on a fading
                    // ally (raising its HP before it dissolves), but it does not
                    // rescue it. This is intentional and final.
                }
            }
            Event::EffectDamaged { tile, amount } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.hp -= amount;
                }
            }
            Event::DamageRedirected {
                from_tile,
                to_tile,
                amount,
                consume,
            } => {
                if let Some(sp) = self.board[*to_tile as usize].spirit.as_mut() {
                    sp.hp -= amount;
                }
                if *consume && let Some(sp) = self.board[*from_tile as usize].spirit.as_mut() {
                    sp.replacement_used = true;
                }
            }
            Event::KeywordGranted {
                tile,
                keyword,
                until_round,
            } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.kw_grants.push((*keyword, *until_round));
                }
            }
            Event::SpiritsSwapped { a, b } => {
                // Only the spirits move; terrain/impression/faded stay with the tile.
                let sa = self.board[*a as usize].spirit.take();
                let sb = self.board[*b as usize].spirit.take();
                self.board[*a as usize].spirit = sb;
                self.board[*b as usize].spirit = sa;
            }
            Event::EngageRestricted { tile, until_round } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.no_engage_until = *until_round;
                }
            }
            Event::FadeDelayed { tile } => {
                if !self.fade_delayed.contains(tile) {
                    self.fade_delayed.push(*tile);
                }
            }
            Event::FadeDelayConsumed { tile } => {
                if let Some(pos) = self.fade_delayed.iter().position(|&t| t == *tile) {
                    self.fade_delayed.swap_remove(pos);
                }
            }
            Event::FlowAnimaScheduled { seat, amount } => {
                self.pending_flow_anima[*seat as usize] =
                    self.pending_flow_anima[*seat as usize].saturating_add(*amount);
            }
            Event::FlowAnimaPaid { seat } => {
                let owed = self.pending_flow_anima[*seat as usize];
                self.player_mut(*seat).anima = self.player_mut(*seat).anima.saturating_add(owed);
                self.pending_flow_anima[*seat as usize] = 0;
            }
            Event::EvolveImprintFreed { seat } => {
                self.ignore_imprint_this_turn[*seat as usize] = true;
            }
            Event::SpiritReclaimed { tile } => {
                self.board[*tile as usize].spirit = None; // leaves, no impression
            }
            Event::NextArrivalBuffed { seat, attack } => {
                self.next_arrival_atk[*seat as usize] += attack;
            }
            Event::NextArrivalConsumed { seat } => {
                self.next_arrival_atk[*seat as usize] = 0;
            }
            Event::NextArrivalSecondEngage { seat, penalty } => {
                self.next_arrival_2nd_engage[*seat as usize] = Some(*penalty);
            }
            Event::NextArrivalSecondConsumed { seat } => {
                self.next_arrival_2nd_engage[*seat as usize] = None;
            }
            Event::FabricationPeeked { seat, tile, card } => {
                let known = &mut self.peeked_fabs[*seat as usize];
                if !known.iter().any(|&(t, c)| t == *tile && c == *card) {
                    known.push((*tile, *card));
                }
            }
            Event::RitualExtraTargetsArmed { count } => {
                self.ritual_extra_targets = *count;
            }
            Event::RitualExtraTargetsConsumed => {
                self.ritual_extra_targets = 0;
            }
            Event::ThroughlineCompleted {
                tile,
                attack,
                defense,
            } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.attack += attack;
                    sp.defense += defense;
                    sp.hp = sp.hp_max;
                    sp.throughline_done = true;
                }
            }
            Event::EffectStat {
                tile,
                attack,
                defense,
                form,
            } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.attack += attack;
                    sp.defense += defense;
                    sp.hp_max += form;
                    sp.hp += form;
                }
            }
            Event::SpiritBecameFading { tile, banished_by } => {
                // The standing-Faded window. A combat banish (`banished_by` is
                // Some) does NOT dissolve here; it stands Faded and lingers until the
                // END of its owner's next turn (the dissolve fires in `end_turn`),
                // giving the owner one Main to Primal-evolve it. Stamp the deadline:
                // the round of the owner's next turn-end, computed deterministically
                // from the round, the acting seat, and the owner (a round-trip-safe
                // function of public state — no entropy, no leak). On round 12 there is
                // no such next turn, so the deadline is never reached in `end_turn` —
                // the base instead lingers standing-Faded through the rest of the round
                // and is dissolved by the Nightfall `finish` pass before scoring (§0.5).
                // An *uncontested* fade (`banished_by` None — the Dusk's sweep) keeps
                // `fade_deadline` None and dissolves at the turn-START Fade step.
                let (round, active) = (self.round, self.active);
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.fading = true;
                    sp.banished_by = *banished_by;
                    sp.fade_deadline =
                        banished_by.map(|_| fade_deadline_round(round, active, sp.owner));
                    // The Throughline buff is once-per-body and FADING BREAKS IT (§5.4):
                    // a body that dissolves to 0 forfeits its completion, so a rescued or
                    // re-formed spirit (a Primal off this Fading base, or a Devolve back to
                    // base) may earn the Throughline anew. Fabled — which leaps from a
                    // HEALTHY base that never faded — is the only path that keeps it.
                    sp.throughline_done = false;
                }
            }
            Event::Overwrote {
                seat,
                card,
                tile,
                success,
                damage_to_defender,
                attack,
                defense,
                attacker_hp_left,
                attacker_hp_max,
                ..
            } => {
                {
                    let p = self.player_mut(*seat);
                    if let Some(i) = p.hand.iter().position(|c| c == card) {
                        p.hand.remove(i);
                    }
                }
                let idx = *tile as usize;
                if *success {
                    // The banished one's mark beneath the newcomer — faction-aware (Solace tallies).
                    self.lay_mark(idx, *seat);
                    self.moved_this_turn.push(*tile); // The overwriter arrived — summoning-sick
                    let t = &mut self.board[idx];
                    t.spirit = Some(Spirit {
                        replacement_used: false,
                        holding: false,
                        face_down: false,
                        is_token: false,
                        placed_by: None,
                        card: *card,
                        owner: *seat,
                        attack: *attack,
                        defense: *defense,
                        hp: *attacker_hp_left,
                        hp_max: *attacker_hp_max,
                        fading: *attacker_hp_left <= 0,
                        banished_by: if *attacker_hp_left <= 0 {
                            Some(seat.other())
                        } else {
                            None
                        },
                        intercepted_this_round: false,
                        traits_stripped: false,
                        traits_stripped_until: None,
                        kw_grants: Vec::new(),
                        no_engage_until: 0,
                        throughline_done: false,
                        copied_reach: None,
                        // An overwriter that arrives spent (≤0 HP) was banished in
                        // the exchange — it stands Faded and lingers to its owner's
                        // next turn-end (this is the owner's own turn, so the deadline
                        // is next round). A surviving overwriter never fades, so None.
                        fade_deadline: if *attacker_hp_left <= 0 {
                            Some(fade_deadline_round(self.round, self.active, *seat))
                        } else {
                            None
                        },
                    });
                } else if let Some(sp) = self.board[idx].spirit.as_mut() {
                    sp.hp -= damage_to_defender; // the attacker dissolved; the wound stays
                }
                if *seat == Seat::A {
                    self.player_a.first_placement_done = true;
                }
            }
            Event::SpiritMoved { from, to } => {
                let sp = self.board[*from as usize].spirit.take();
                self.board[*to as usize].spirit = sp;
                self.moved_this_turn.push(*to); // This spirit has spent its one Move this turn
            }
            Event::SpiritDissolved { tile, impression } => {
                // A fully-dissolved spirit ticks its owner's this-turn dissolution count
                // (Remember Them) and — if non-token — joins its owner's Recover pool.
                if let Some(sp) = &self.board[*tile as usize].spirit {
                    self.dissolved_this_turn[sp.owner as usize] += 1;
                    if !sp.is_token {
                        self.dissolved.push((sp.owner, sp.card));
                    }
                }
                self.board[*tile as usize].spirit = None;
                self.lay_mark(*tile as usize, *impression);
            }
            Event::ControlTaken { tile, new_owner } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.owner = *new_owner;
                    sp.placed_by = None;
                }
            }
            Event::TraitsStripped { tile } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.traits_stripped = true;
                }
            }
            Event::TraitsStrippedUntil { tile, until_round } => {
                if let Some(sp) = self.board[*tile as usize].spirit.as_mut() {
                    sp.traits_stripped_until = Some(*until_round);
                }
            }
            Event::RitualDiscountGranted { seat, amount } => {
                self.next_ritual_discount[*seat as usize] += *amount;
            }
            Event::RitualDiscountConsumed { seat } => {
                self.next_ritual_discount[*seat as usize] = 0;
            }
            Event::CardTaxed {
                seat,
                amount,
                until_round,
            } => {
                self.card_tax[*seat as usize] = (*amount, *until_round);
            }
            Event::CardTaxSpent { seat } => {
                self.card_tax[*seat as usize] = (0, 0);
            }
            Event::RecoverTaken { seat, card } => {
                self.pending_choice = None;
                self.player_mut(*seat).hand.push(*card);
                if let Some(pos) = self
                    .dissolved
                    .iter()
                    .position(|(s, c)| s == seat && c == card)
                {
                    self.dissolved.remove(pos);
                }
                self.resume_after_choice();
            }
            Event::TurnEnded { seat } => {
                self.player_mut(*seat).glimpsed_this_turn = false;
                // Bearer of Small Stones: the "this turn" evolution exception expires.
                self.ignore_imprint_this_turn[*seat as usize] = false;
                // Kindle / Again!: unused next-arrival offers lapse when the turn ends.
                self.next_arrival_atk[*seat as usize] = 0;
                self.next_arrival_2nd_engage[*seat as usize] = None;
                if self.is_2v2() {
                    self.active_slot = self.active_slot.next_2v2();
                    self.active = self.active_slot.team();
                } else {
                    self.active = seat.other();
                    self.active_slot = if self.active == Seat::A {
                        crate::types::SeatSlot::A1
                    } else {
                        crate::types::SeatSlot::B1
                    };
                }
                self.phase = Phase::Acting;
                // Each spirit's one free Move (and arrival summoning-sickness) resets for the
                // new turn.
                self.moved_this_turn.clear();
                // The incoming seat's this-turn dissolution count starts fresh; its own
                // combat banishes + its turn-END Fade will tick it back up. Read by
                // Remember Them.
                self.dissolved_this_turn[self.active as usize] = 0;
            }
            Event::RoundAdvanced { round } => {
                let r = *round;
                self.temp_mods.retain(|m| m.until_round >= r);
                self.temp_reach.retain(|m| m.until_round >= r);
                self.temp_retaliation.retain(|&(_, _, until)| until >= r);
                self.temp_restrict.retain(|m| m.until_round >= r);
                for tax in self.card_tax.iter_mut() {
                    if tax.1 < r {
                        *tax = (0, 0);
                    }
                }
                self.round = *round;
                self.unwriting_told_this_round = false;
                for t in self.board.iter_mut() {
                    if let Some(sp) = t.spirit.as_mut() {
                        sp.intercepted_this_round = false;
                        // This-round keyword grants (Dig In) expire; permanent
                        // grants (u8::MAX, The Long Watch) survive.
                        sp.kw_grants.retain(|(_, until)| *until >= r);
                        // Smear's this-round trait blanking lifts when its round passes.
                        if sp.traits_stripped_until.is_some_and(|u| u < r) {
                            sp.traits_stripped_until = None;
                        }
                    }
                }
            }
            Event::TileFaded { tile } => {
                self.board[*tile as usize].faded = true;
            }
            Event::MemoryContracted { faded_tiles } => {
                // The Dusk is INSTANT and decoupled from Fade (§0.5/§5): at the Curl the
                // empty rim darkens AND the Unwritten on the now-dark rim dissolve at
                // ONCE — no window, no deferred fade. The only spirits in `faded_tiles`
                // are rim Unwritten (the decide-side filter is `is_antagonist_creature`;
                // a player's standing spirit is HELD, never swept), and the Unwritten
                // leave **nothing** — no body, no impression, no tally (the Solace-no-mark
                // rule). So this handler stays a pure mechanical application: mark each
                // tile faded and remove any swept Unwritten outright. (No fading body is
                // ever left behind, so every Fading spirit anywhere carries a
                // `fade_deadline` — the strengthened invariant.)
                self.contracted = true;
                for t in faded_tiles {
                    let tile = &mut self.board[*t as usize];
                    tile.faded = true;
                    // Dissolve the swept Unwritten immediately — it leaves nothing.
                    if tile.spirit.is_some() {
                        tile.spirit = None;
                    }
                    // Sweep any terrain off the darkening tile too: §5 darkens every EMPTY
                    // rim tile, and the Held Ground law holds a standing *spirit*'s tile,
                    // not bare infrastructure. A Landmark / face-down Fabrication on a
                    // body-less rim tile goes dark with the ground it sat on — without this
                    // the tile kept its terrain, violating the "no terrain on a faded tile"
                    // invariant (a faded tile holds nothing). (The decide-side filter only
                    // ever lists empty-of-spirit or Unwritten rim tiles, so a player's HELD
                    // standing spirit — which keeps its tile and its terrain-free tile both —
                    // is never in `faded_tiles`.)
                    tile.terrain = None;
                }
            }
            Event::MatchEnded {
                result,
                score_a,
                score_b,
            } => {
                self.phase = Phase::Finished {
                    result: *result,
                    score_a: *score_a,
                    score_b: *score_b,
                };
            }
            // Absence forfeit: the present player wins, by forfeit not score.
            Event::MatchAbandoned {
                seat,
                score_a,
                score_b,
            } => {
                self.phase = Phase::Finished {
                    result: MatchResult::Win(seat.other()),
                    score_a: *score_a,
                    score_b: *score_b,
                };
            }
            // Mulligan (§5): the seat's hand + deck were recomputed in `decide`
            // (reshuffle, draw a fresh full hand, bottom one). Apply them verbatim
            // — no entropy here (the draws were journaled in `decide`) — and mark
            // the once-per-match mulligan spent (the public beat the view shows).
            // 1v1-only by gate, so `player_mut(seat)` is the right player.
            Event::Mulliganed { seat, hand, deck } => {
                let p = self.player_mut(*seat);
                p.hand = hand.clone();
                p.deck = deck.clone();
                p.peeked_top = None; // a fresh page invalidates any prior Glimpse peek
                self.mulliganed[*seat as usize] = true;
            }
        }
    }
}
