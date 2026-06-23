//! Match settings for hosting a match (design: website_plan.md "Match settings").
//! A host chooses the mode, who fills each seat (human or a bot of some difficulty),
//! and the faction fought against. Parsed from the `POST /matches` query, generalizing
//! the legacy `mode=2v2` / `opponent=bot` / `difficulty=` params so old callers keep
//! working. Kept in its own module so `create_match` stays lean.
use recollect_bot::{Difficulty, Faction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    OneVsOne,
    TwoVsTwo,
}

impl Mode {
    pub(crate) fn seat_count(self) -> usize {
        match self {
            Mode::OneVsOne => 2,
            Mode::TwoVsTwo => 4,
        }
    }
}

/// Who fills a seat. Human seats get a join token; bot seats are server-driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeatFill {
    Human,
    Bot(Difficulty),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MatchSettings {
    pub mode: Mode,
    /// In seat order — `[A, B]` for 1v1, `[A1, B1, A2, B2]` for 2v2.
    pub seats: Vec<SeatFill>,
    /// The faction fought against (the opponent bots play as this).
    pub opponent_faction: Faction,
    /// Optional explicit match seed (`?seed=N`): reproducible matches + deterministic tests.
    /// `None` ⇒ OS entropy. Replaces the old `RECOLLECT_MATCH_SEED` env (no global/unsafe state).
    pub seed: Option<u64>,
    /// Absence-forfeit grace (`?abandon_grace_secs=N`): how long a HUMAN seat/slot may stay
    /// disconnected before the server issues `Command::MatchAbandoned` against it. `None` ⇒ the
    /// default ([`DEFAULT_ABANDON_GRACE`]); `0` DISABLES the forfeit (a drop never ends the match —
    /// what the reconnect tests rely on). Only human seats arm; a bot has no socket and never forfeits.
    pub abandon_grace: Option<std::time::Duration>,
}

/// The default absence-forfeit grace window: a human seat/slot that disconnects and does not
/// reconnect within this long forfeits (2v2: any one absent slot forfeits its whole team). 120s —
/// long enough to ride out a flaky network or a tab reload, short enough that an abandoned table
/// frees its opponent promptly. Overridable per match via `?abandon_grace_secs=N` (`0` disables it).
pub(crate) const DEFAULT_ABANDON_GRACE: std::time::Duration = std::time::Duration::from_secs(120);

impl MatchSettings {
    /// Parse the create-match query. Backwards-compatible with the legacy params
    /// (`mode=2v2`, `opponent=bot`, `difficulty=…`); the new form is explicit
    /// `seats=human,bot:hard,…` plus `faction=lorekeeper|solace`.
    pub(crate) fn from_query(q: &str) -> Result<MatchSettings, String> {
        let val = |key: &str| q.split('&').find_map(|kv| kv.strip_prefix(key));
        let flag = |kv: &str| q.split('&').any(|p| p == kv);

        let mode = if flag("mode=2v2") {
            Mode::TwoVsTwo
        } else {
            Mode::OneVsOne
        };
        let opponent_faction = match val("faction=") {
            None => Faction::default(),
            Some("lorekeeper") | Some("lorekeepers") => Faction::Lorekeeper,
            Some("solace") => Faction::Solace,
            Some(other) => return Err(format!("unknown faction '{other}'")),
        };
        let seats = match val("seats=") {
            Some(list) => list
                .split(',')
                .map(parse_seat)
                .collect::<Result<Vec<_>, _>>()?,
            None => match mode {
                // Legacy 1v1: seat A human; seat B a bot iff `opponent=bot`.
                Mode::OneVsOne => {
                    let b = if flag("opponent=bot") {
                        SeatFill::Bot(
                            val("difficulty=")
                                .map(super::parse_difficulty)
                                .unwrap_or_default(),
                        )
                    } else {
                        SeatFill::Human
                    };
                    vec![SeatFill::Human, b]
                }
                // Legacy 2v2: a four-human lobby (code-join all round).
                Mode::TwoVsTwo => vec![SeatFill::Human; 4],
            },
        };
        if seats.len() != mode.seat_count() {
            return Err(format!(
                "{mode:?} needs {} seats, got {}",
                mode.seat_count(),
                seats.len()
            ));
        }
        // Explicit `?seed=N` for reproducible matches + deterministic tests (no env/unsafe).
        let seed = val("seed=").and_then(|s| s.parse::<u64>().ok());
        // Absence-forfeit grace: `?abandon_grace_secs=N` ⇒ `Some(N s)` (0 disables the forfeit);
        // absent ⇒ `None` (the host default). A non-numeric value falls back to the default rather
        // than refusing match creation (a forfeit dial is non-essential to starting a game).
        let abandon_grace = val("abandon_grace_secs=")
            .and_then(|s| s.parse::<u64>().ok())
            .map(std::time::Duration::from_secs);
        Ok(MatchSettings {
            mode,
            seats,
            opponent_faction,
            seed,
            abandon_grace,
        })
    }
}

fn parse_seat(s: &str) -> Result<SeatFill, String> {
    let s = s.trim();
    if s == "human" {
        return Ok(SeatFill::Human);
    }
    if let Some(rest) = s.strip_prefix("bot") {
        let d = rest.strip_prefix(':').unwrap_or("");
        return Ok(SeatFill::Bot(if d.is_empty() {
            Difficulty::default()
        } else {
            super::parse_difficulty(d)
        }));
    }
    Err(format!(
        "unknown seat '{s}' (want human | bot | bot:easy|normal|hard)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_1v1_vs_bot() {
        let s = MatchSettings::from_query("opponent=bot&difficulty=hard").unwrap();
        assert_eq!(s.mode, Mode::OneVsOne);
        assert_eq!(
            s.seats,
            vec![SeatFill::Human, SeatFill::Bot(Difficulty::Hard)]
        );
    }

    #[test]
    fn legacy_2v2_is_four_humans() {
        let s = MatchSettings::from_query("mode=2v2").unwrap();
        assert_eq!(s.mode, Mode::TwoVsTwo);
        assert_eq!(s.seats, vec![SeatFill::Human; 4]);
    }

    #[test]
    fn explicit_2v2_mixed_seats_and_faction() {
        let s = MatchSettings::from_query(
            "mode=2v2&seats=human,bot:easy,bot:hard,human&faction=solace",
        )
        .unwrap();
        assert_eq!(s.opponent_faction, Faction::Solace);
        assert_eq!(
            s.seats,
            vec![
                SeatFill::Human,
                SeatFill::Bot(Difficulty::Easy),
                SeatFill::Bot(Difficulty::Hard),
                SeatFill::Human
            ]
        );
    }

    #[test]
    fn rejects_wrong_seat_count() {
        assert!(MatchSettings::from_query("seats=human").is_err()); // 1v1 needs 2
        assert!(MatchSettings::from_query("mode=2v2&seats=human,human").is_err()); // 2v2 needs 4
    }

    #[test]
    fn rejects_unknown_faction_or_seat() {
        assert!(MatchSettings::from_query("faction=goblin").is_err());
        assert!(MatchSettings::from_query("seats=wizard,human").is_err());
    }

    #[test]
    fn parses_the_abandon_grace_override() {
        use std::time::Duration;
        // Absent ⇒ None (the host applies DEFAULT_ABANDON_GRACE).
        assert_eq!(MatchSettings::from_query("").unwrap().abandon_grace, None);
        // An explicit positive value ⇒ that many seconds.
        assert_eq!(
            MatchSettings::from_query("abandon_grace_secs=45")
                .unwrap()
                .abandon_grace,
            Some(Duration::from_secs(45))
        );
        // 0 is meaningful: Some(ZERO) DISABLES the forfeit (distinct from None's default).
        assert_eq!(
            MatchSettings::from_query("abandon_grace_secs=0")
                .unwrap()
                .abandon_grace,
            Some(Duration::ZERO)
        );
        // A non-numeric value is ignored (falls back to the default), not an error.
        assert_eq!(
            MatchSettings::from_query("abandon_grace_secs=soon")
                .unwrap()
                .abandon_grace,
            None
        );
    }
}
