//! Wire protocol. Every message carries `v` so we can evolve without breaking
//! live matches. Servers reject unknown majors; clients surface a forced
//! update. Commands carry a client sequence number for idempotency and
//! anti-replay; the server echoes it in acks.
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
use recollect_core::state::Command;
use recollect_core::types::{Seat, SeatSlot};
use recollect_core::view::{PlayerView, TeamView};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    /// First message on the socket. `match_token` is a short-lived, per-seat
    /// credential minted at match creation — never a long-lived account token.
    Hello {
        v: u16,
        match_token: String,
        /// Optional session display name — no account needed; shown to the table.
        #[serde(default)]
        name: Option<String>,
        /// Anonymous, opaque per-client id (persisted client-side) that links a
        /// player's matches for journey analytics — no account, no PII.
        #[serde(default)]
        session_id: Option<String>,
    },
    Cmd {
        v: u16,
        seq: u64,
        command: Command,
    },
    Ping {
        v: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        v: u16,
        seat: Seat,
        view: PlayerView,
        /// The recipient's legal moves (empty when it isn't their turn). Optional
        /// on the wire — older clients ignore it, older servers omit it.
        #[serde(default)]
        legal: Vec<LegalMove>,
        /// Session display names by seat (index = seat). Empty/None where unset.
        #[serde(default)]
        seat_names: Vec<Option<String>>,
    },
    /// Acks carry the seq they answer. State arrives as the caller's own view
    /// (never the raw state, never the opponent's view).
    Applied {
        v: u16,
        seq: u64,
        view: PlayerView,
        #[serde(default)]
        legal: Vec<LegalMove>,
    },
    Rejected {
        v: u16,
        seq: u64,
        reason: String,
    },
    /// Pushed to the *other* seat after any state change.
    Update {
        v: u16,
        view: PlayerView,
        #[serde(default)]
        legal: Vec<LegalMove>,
    },
    /// 2v2: the slot's four-seat view on connect. `TeamView` redacts
    /// exactly as `PlayerView` does — your hand, everyone else as counts.
    TeamWelcome {
        v: u16,
        slot: SeatSlot,
        view: TeamView,
        /// The slot's legal moves (empty unless it's this slot's turn).
        #[serde(default)]
        legal: Vec<LegalMove>,
    },
    /// 2v2: the ack to the acting slot, carrying its own fresh TeamView.
    TeamApplied {
        v: u16,
        seq: u64,
        slot: SeatSlot,
        view: TeamView,
        #[serde(default)]
        legal: Vec<LegalMove>,
    },
    /// 2v2: pushed to each *other* slot after a state change.
    TeamUpdate {
        v: u16,
        slot: SeatSlot,
        view: TeamView,
        #[serde(default)]
        legal: Vec<LegalMove>,
    },
    Pong {
        v: u16,
    },
    /// Commit–reveal: pushed to each seat when the telling ends, disclosing
    /// the match seed and the salt behind the commitment published at creation
    /// (the POST response's `seed_commit`). A client recomputes
    /// `SHA-256(seed_le ‖ salt)`, checks it equals `commit`, then replays its
    /// command log against `seed` — proof the shuffle was fixed before play and
    /// neither side could rig it. The seed is disclosed ONLY here, at the end; it
    /// is never in a view or an in-flight event, so the determinism + redaction
    /// invariants hold during play.
    SeedRevealed {
        v: u16,
        seed: u64,
        salt_hex: String,
        commit_hex: String,
    },
    Error {
        v: u16,
        message: String,
    },
}

/// A legal command for the active seat, with a human label. The server (which
/// holds the engine) computes these and ships them alongside the view, so a
/// networked client — which has only the redacted `PlayerView`, no engine —
/// can offer the same legal-move menu a local client does. Engagement moves also
/// carry a structured [`ForecastView`] so a frontend can render the combat
/// outcome as data (numbers/icons), not only the text baked into `label`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegalMove {
    pub label: String,
    pub cmd: Command,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forecast: Option<ForecastView>,
}

/// The combat forecast for an engagement, as wire data — a serializable mirror of
/// the core's `Forecast` (which lives in the effects-owned engine, so it isn't
/// serialized there). `to_defender`/`to_attacker` are the damage each takes;
/// the `banishes_*` flags whether it falls; `*_echo_live` whether an Echo could
/// fire (a 20% +20 swing).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ForecastView {
    pub to_defender: i16,
    pub to_attacker: i16,
    pub banishes_defender: bool,
    pub banishes_attacker: bool,
    pub attacker_echo_live: bool,
    pub defender_echo_live: bool,
}

// --- The canonical command labeler -------------------------------------------
// One gloss per command, in one place: the server renders the wire `LegalMove`s
// through `label`, and the CLI's local TUI `describe` delegates here too. Combat
// forecasts ride engagement labels. (The web's local LocalGame keeps its own
// wasm labeler.) A new `Command` variant gets its label here.
use recollect_core::Engine;
use recollect_core::engine::{Forecast, forecast_exchange};
use recollect_core::types::{CardDef, tile_xy};

/// Human gloss for a legal command from `seat`'s vantage.
pub fn label(engine: &Engine, seat: Seat, cmd: &Command) -> String {
    let st = engine.state();
    let hand = |i: &u8| engine.card(st.player(seat).hand[*i as usize]).name.clone();
    match cmd {
        Command::MatchAbandoned { seat: who } => {
            format!("{who:?} abandons the match (forfeit)")
        }
        Command::Mulligan { .. } => {
            "Mulligan (redraw a fresh hand, bottom one — once, at the opening)".into()
        }
        Command::Glimpse => "Glimpse (burn a hand card, then peek your top card)".into(),
        Command::EndTurn => "End turn".into(),
        Command::TellUnwriting { .. } => "Tell an Unwriting".into(),
        Command::Choose { index } => {
            // Glimpse (§5) raises two choices; gloss each with the card in play so the
            // menu is legible (`Choose option N` is opaque). Both are owner-visible, and
            // this labeler runs only for the choosing seat. Other choice kinds keep the
            // generic gloss.
            match &st.pending_choice {
                // Step 1 — the BURN cost: which hand card to spend.
                Some(recollect_core::state::PendingChoice::GlimpseBurn { burnable, .. }) => {
                    match burnable.get(*index as usize) {
                        Some(&id) => format!("Burn {} to glimpse", engine.card(id).name),
                        None => format!("Choose option {index}"),
                    }
                }
                // Step 2 — keep or bottom the peeked top.
                Some(recollect_core::state::PendingChoice::Glimpse { top, .. }) => {
                    let top = engine.card(*top).name.clone();
                    match index {
                        0 => format!("Keep {top} on top"),
                        1 => format!("Bottom {top} for +1 anima"),
                        _ => format!("Choose option {index}"),
                    }
                }
                _ => format!("Choose option {index}"),
            }
        }
        Command::SetOrders { tile, hold } => {
            format!(
                "Orders: tile {tile} {}",
                if *hold { "Hold" } else { "Watch" }
            )
        }
        Command::Reveal { tile, engage } => format!(
            "Reveal lurker at {tile}{}",
            engage
                .map(|t| format!(", striking {t}"))
                .unwrap_or_default()
        ),
        Command::CastRitual { hand_index } => format!("Cast ritual (hand {hand_index})"),
        Command::AttachBond {
            hand_index,
            tile_a,
            tile_b,
        } => format!("Bond {tile_a}+{tile_b} (hand {hand_index})"),
        Command::PlaceLandmark { hand_index, tile } => {
            format!("Place landmark at {tile} (hand {hand_index})")
        }
        Command::SetFabrication { hand_index, tile } => {
            format!("Set fabrication at {tile} (hand {hand_index})")
        }
        Command::Evolve {
            tile,
            form_hand,
            fuel,
            engage,
        } => {
            let how = match fuel {
                Some(d) => format!(" fueled by {d}"),
                None => " (primal surge)".to_string(),
            };
            let hit = match engage {
                Some(t) => format!(", striking {t}"),
                None => String::new(),
            };
            format!("Play form (hand {form_hand}) onto base at {tile}{how}{hit}")
        }
        Command::Devolve { tile, base_hand } => {
            // Vocabulary (law): the Lorekeeper REVERTS, the Solace RECEDES — one engine
            // action, the faction's verb in player-facing text.
            let verb = match st.rules.factions[seat as usize] {
                recollect_core::types::Faction::Solace => "Recede",
                recollect_core::types::Faction::Lorekeeper => "Revert",
            };
            format!(
                "{verb} the faded form at {tile} to {} (hand {base_hand}) — the rescue",
                hand(base_hand)
            )
        }
        Command::BanishStray => "Banish the surfaced Stray".to_string(),
        Command::StrikeFabrication { from, tile } => {
            format!("Strike the lie at {tile} from {from}")
        }
        Command::Release { hand_index } => {
            format!("Release {} to the bottom of the page", hand(hand_index))
        }
        Command::PlaySpirit {
            hand_index,
            tile,
            engage: None,
            ..
        } => format!("Play {} → {}", hand(hand_index), tile_name(*tile)),
        Command::PlaySpirit {
            hand_index,
            tile,
            engage: Some(t),
            ..
        } => {
            let ac = engine.card(st.player(seat).hand[*hand_index as usize]);
            format!(
                "Play {} → {} ENGAGING {}{}",
                ac.name,
                tile_name(*tile),
                at(engine, *t),
                fc(engine, ac, *t)
            )
        }
        Command::Overwrite { hand_index, tile } => {
            let ac = engine.card(st.player(seat).hand[*hand_index as usize]);
            format!(
                "OVERWRITE {} with {}{}",
                at(engine, *tile),
                ac.name,
                fc(engine, ac, *tile)
            )
        }
        Command::MoveSpirit { from, to, engage } => {
            let mut s = format!(
                "Step {} {}→{}",
                at(engine, *from),
                tile_name(*from),
                tile_name(*to)
            );
            if let Some(t) = engage
                && let Some(sp) = st.spirit_at(*from)
            {
                let ac = engine.card(sp.card);
                s.push_str(&format!(
                    " engaging {}{}",
                    at(engine, *t),
                    fc_spirit(engine, sp, ac, *t)
                ));
            }
            s
        }
        Command::Reclaim { tile } => {
            format!("Reclaim {} (cash it back for Anima)", at(engine, *tile))
        }
    }
}

/// The structured combat forecast for a command's engagement, from `seat`'s
/// vantage — `Some` for the engage-bearing moves (PlaySpirit-engage, Overwrite,
/// MoveSpirit-engage), `None` otherwise. Same math as the forecast text baked
/// into [`label`]; this returns it as data for a frontend to render.
pub fn forecast(engine: &Engine, seat: Seat, cmd: &Command) -> Option<ForecastView> {
    let st = engine.state();
    let f = match cmd {
        Command::PlaySpirit {
            hand_index,
            engage: Some(t),
            ..
        } => {
            let ac = engine.card(*st.player(seat).hand.get(*hand_index as usize)?);
            let d = st.spirit_at(*t)?;
            forecast_exchange(
                ac,
                ac.attack,
                ac.defense,
                ac.hp,
                ac.hp,
                d,
                engine.card(d.card),
                0,
                engine.warded_at(*t),
            )
        }
        Command::Overwrite { hand_index, tile } => {
            let ac = engine.card(*st.player(seat).hand.get(*hand_index as usize)?);
            let d = st.spirit_at(*tile)?;
            forecast_exchange(
                ac,
                ac.attack,
                ac.defense,
                ac.hp,
                ac.hp,
                d,
                engine.card(d.card),
                0,
                engine.warded_at(*tile),
            )
        }
        Command::MoveSpirit {
            from,
            engage: Some(t),
            ..
        } => {
            let sp = st.spirit_at(*from)?;
            let ac = engine.card(sp.card);
            let d = st.spirit_at(*t)?;
            forecast_exchange(
                ac,
                sp.attack,
                sp.defense,
                sp.hp,
                sp.hp_max,
                d,
                engine.card(d.card),
                0,
                engine.warded_at(*t),
            )
        }
        _ => return None,
    };
    Some(ForecastView {
        to_defender: f.to_defender,
        to_attacker: f.to_attacker,
        banishes_defender: f.banishes_defender,
        banishes_attacker: f.banishes_attacker,
        attacker_echo_live: f.attacker_echo_live,
        defender_echo_live: f.defender_echo_live,
    })
}

fn fc(engine: &Engine, ac: &CardDef, t: u8) -> String {
    let Some(dfn) = engine.state().spirit_at(t) else {
        return String::new();
    };
    let dc = engine.card(dfn.card);
    fmt_forecast(&forecast_exchange(
        ac,
        ac.attack,
        ac.defense,
        ac.hp,
        ac.hp,
        dfn,
        dc,
        0,
        engine.warded_at(t),
    ))
}

fn fc_spirit(engine: &Engine, sp: &recollect_core::state::Spirit, ac: &CardDef, t: u8) -> String {
    let Some(dfn) = engine.state().spirit_at(t) else {
        return String::new();
    };
    let dc = engine.card(dfn.card);
    fmt_forecast(&forecast_exchange(
        ac,
        sp.attack,
        sp.defense,
        sp.hp,
        sp.hp_max,
        dfn,
        dc,
        0,
        engine.warded_at(t),
    ))
}

fn fmt_forecast(f: &Forecast) -> String {
    format!(
        "  [deal {}{} · take {}{}{}{}]",
        f.to_defender,
        if f.banishes_defender { " BANISH" } else { "" },
        f.to_attacker,
        if f.banishes_attacker {
            " — you'd be banished"
        } else {
            ""
        },
        if f.defender_echo_live {
            " · their Echo live (20%: +20)"
        } else {
            ""
        },
        if f.attacker_echo_live {
            " · your Echo live"
        } else {
            ""
        },
    )
}

fn at(engine: &Engine, t: u8) -> String {
    match engine.state().spirit_at(t) {
        Some(s) => format!("{} ({})", engine.card(s.card).name, tile_name(t)),
        None => tile_name(t),
    }
}

fn tile_name(t: u8) -> String {
    let (x, y) = tile_xy(t);
    format!("{}{}", (b'a' + x as u8) as char, y + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trips_and_is_tagged() {
        let msg = ClientMsg::Cmd {
            v: PROTOCOL_VERSION,
            seq: 7,
            command: Command::Glimpse,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"t\":\"cmd\""), "{json}");
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        match back {
            ClientMsg::Cmd { seq, .. } => assert_eq!(seq, 7),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn team_messages_carry_a_team_view_over_the_wire() {
        use recollect_core::Engine;
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::{CardId, CardKind, SeatSlot};
        use recollect_core::view::view_for_slot;

        let cat = canon_catalog();
        let deck: Vec<CardId> = cat
            .iter()
            .filter(|c| c.kind == CardKind::Spirit)
            .take(20)
            .map(|c| c.id)
            .collect();
        let (e, _) = Engine::new_2v2(1, cat, [deck.clone(), deck.clone(), deck.clone(), deck]);
        let view = view_for_slot(&e, SeatSlot::A1);

        let msg = ServerMsg::TeamWelcome {
            v: PROTOCOL_VERSION,
            slot: SeatSlot::A1,
            view,
            legal: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains("\"t\":\"team_welcome\""),
            "tagged: {json:.80}"
        );
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            back,
            ServerMsg::TeamWelcome {
                slot: SeatSlot::A1,
                ..
            }
        ));
    }

    #[test]
    fn the_view_carries_the_cue_fields_to_a_networked_client() {
        // A networked client has no engine — the rules-change cues + the Solace's
        // live score must ride the wire view. Assert the three fields survive a
        // ServerMsg round-trip (the protocol re-exports the core view, so threading is
        // structural, but this pins it so a future view change can't drop them silently).
        use recollect_core::Engine;
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::CardId;
        use recollect_core::view::view_for;

        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..10u16).chain(0..10u16).map(CardId).collect();
        let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
        // Place a spirit so a SpiritView (carrying `mobile`) exists on the board.
        let play = e
            .legal_commands(Seat::A)
            .into_iter()
            .find(|c| matches!(c, Command::PlaySpirit { engage: None, .. }))
            .expect("an opening placement is legal");
        let Command::PlaySpirit { tile: placed, .. } = play else {
            unreachable!()
        };
        e.apply(Seat::A, play).unwrap();
        let view = view_for(&e, Seat::A);
        let msg = ServerMsg::Welcome {
            v: PROTOCOL_VERSION,
            seat: Seat::A,
            view,
            legal: vec![],
            seat_names: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ServerMsg = serde_json::from_str(&json).unwrap();
        let ServerMsg::Welcome { view, .. } = back else {
            panic!("round-trips as Welcome");
        };
        // The Solace score crosses the wire (none yet → 0). The per-turn move state
        // crosses too: the just-placed spirit arrived this turn (summoning-sick), so
        // its tile is in `moved_this_turn` — proving the field carries real state,
        // not a default.
        assert_eq!(view.solace_erasures, 0);
        assert!(
            view.moved_this_turn.contains(&placed),
            "the arriving spirit's tile rode the wire in moved_this_turn"
        );
        // And every spirit on the wire carries its Mobile keyword (field present).
        assert!(
            json.contains("\"mobile\""),
            "the per-spirit Mobile keyword crosses the wire: {json:.0}"
        );
    }

    #[test]
    fn label_glosses_commands_for_the_wire_menu() {
        use recollect_core::Engine;
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::CardId;

        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..10u16).chain(0..10u16).map(CardId).collect();
        let (e, _) = Engine::new(7, cat, deck.clone(), deck);
        assert_eq!(label(&e, Seat::A, &Command::EndTurn), "End turn");
        assert!(label(&e, Seat::A, &Command::Glimpse).starts_with("Glimpse"));
        let play = Command::PlaySpirit {
            hand_index: 0,
            tile: 12,
            engage: None,
            chain_prefs: vec![],
        };
        let l = label(&e, Seat::A, &play);
        assert!(
            l.starts_with("Play ") && l.contains(" → "),
            "a play names a card and a tile: {l}"
        );
        // §5: the opening mulligan glosses for the wire menu, and is genuinely in
        // the opener's legal set (the opening window).
        let mull = label(&e, Seat::A, &Command::Mulligan { seat: Seat::A });
        assert!(
            mull.starts_with("Mulligan"),
            "the mulligan glosses for the menu: {mull}"
        );
        assert!(
            e.legal_commands(Seat::A)
                .iter()
                .any(|c| matches!(c, Command::Mulligan { .. })),
            "the opener is offered the mulligan in the opening"
        );
        // Every legal opening move gets a non-empty label (exhaustive over the set).
        for cmd in e.legal_commands(Seat::A) {
            assert!(!label(&e, Seat::A, &cmd).is_empty());
        }
    }

    #[test]
    fn forecast_is_structured_data_for_engagements_only() {
        use recollect_core::Engine;
        use recollect_core::cards::canon_catalog;
        use recollect_core::types::CardId;

        let cat = canon_catalog();
        let deck: Vec<CardId> = (0..10u16).chain(0..10u16).map(CardId).collect();
        let (mut e, _) = Engine::new(7, cat, deck.clone(), deck);
        // Non-engage moves carry no structured forecast.
        assert!(forecast(&e, Seat::A, &Command::EndTurn).is_none());
        assert!(forecast(&e, Seat::A, &Command::Glimpse).is_none());

        // Drive a placement-heavy game; the first engage move must carry a forecast
        // matching the text `label` bakes in (same forecast_exchange math).
        let mut found = false;
        for _ in 0..400 {
            if matches!(
                e.state().phase,
                recollect_core::state::Phase::Finished { .. }
            ) {
                break;
            }
            let seat = e.state().active;
            let legal = e.legal_commands(seat);
            if let Some(eng) = legal.iter().find(|c| {
                matches!(
                    c,
                    Command::PlaySpirit {
                        engage: Some(_),
                        ..
                    } | Command::MoveSpirit {
                        engage: Some(_),
                        ..
                    } | Command::Overwrite { .. }
                )
            }) {
                let f = forecast(&e, seat, eng).expect("an engage move carries a forecast");
                assert!(f.to_defender >= 0, "defender damage is non-negative");
                let banished = if f.banishes_defender { " BANISH" } else { "" };
                assert!(
                    label(&e, seat, eng).contains(&format!("deal {}{}", f.to_defender, banished)),
                    "the label text and the structured forecast agree"
                );
                found = true;
                break;
            }
            // Place a spirit to create adjacency, else take any legal move to advance.
            let cmd = legal
                .iter()
                .find(|c| matches!(c, Command::PlaySpirit { .. }))
                .cloned()
                .unwrap_or_else(|| legal[0].clone());
            e.apply(seat, cmd).unwrap();
        }
        assert!(found, "engage moves arise in a placement game");
    }
}
