//! Headless transport: no terminal UI, for bots, scripts, and CI.
//!
//! - [`autoplay`]: the AI plays one seeded match to a result (optionally an
//!   NDJSON event stream). Single-match — the batch fairness/balance sweeps stay
//!   in `recollect-bot` (fleet/calibrate).
//! - [`protocol`]: a JSON-lines driver. The program controls Seat A (one
//!   `Command` JSON per stdin line), the AI plays Seat B; the driver's
//!   `PlayerView` is emitted as JSON before each of its turns, the result at the
//!   end. Redaction holds — only Seat A's view is ever emitted.
use recollect_bot::Difficulty;
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{generate_deck, offer};
use recollect_core::rng::Rng;
use recollect_core::state::{Command, Event, Phase};
use recollect_core::view::view_for;
use recollect_core::{Engine, Seat};
use std::io::{self, BufRead, Write};

/// Build a both-AI engine from a seed (auto deck styles), like local play with
/// no human picker.
fn new_engine(seed: u64) -> (Engine, Vec<Event>) {
    let catalog = canon_catalog();
    let style_a = offer(seed)[0].id;
    let style_b = offer(seed ^ 0xB)[0].id;
    let deck_a = generate_deck(style_a, seed, &catalog);
    let deck_b = generate_deck(style_b, seed.wrapping_add(1), &catalog);
    Engine::new(seed, catalog, deck_a, deck_b)
}

fn emit_events(events: &[Event]) {
    for ev in events {
        if let Ok(j) = serde_json::to_string(ev) {
            println!("{j}");
        }
    }
}

fn emit_result(seed: u64, phase: &Phase) {
    if let Phase::Finished {
        result,
        score_a,
        score_b,
    } = phase
    {
        println!(
            "{}",
            serde_json::json!({
                "result": format!("{result:?}"),
                "score_a": score_a,
                "score_b": score_b,
                "seed": seed,
            })
        );
    }
}

/// AI vs AI, one seeded match, deterministic. Prints the result JSON; with
/// `ndjson`, every event is a JSON line as it happens.
pub fn autoplay(seed: u64, difficulty: Difficulty, ndjson: bool) {
    let (mut engine, opening) = new_engine(seed);
    let mut ai = Rng::from_seed(seed ^ 0xA1);
    if ndjson {
        emit_events(&opening);
    }
    // The SAME loop the sims run (`recollect_bot::drive_match`) — one path, so headless autoplay and
    // the balance sims can never diverge. We only add the NDJSON event emit on top.
    recollect_bot::drive_match(
        &mut engine,
        |e, seat| recollect_bot::choose(e, seat, difficulty, &mut ai),
        |_, events| {
            if ndjson {
                emit_events(events);
            }
        },
    );
    emit_result(seed, &engine.state().phase);
}

/// JSON-lines protocol over stdin/stdout. The caller drives Seat A; the AI plays
/// Seat B. Each line of stdin is one `Command` (serde JSON); before each Seat-A
/// turn we print Seat A's `PlayerView`, and at the end the result.
pub fn protocol(seed: u64, difficulty: Difficulty) {
    let (mut engine, _opening) = new_engine(seed);
    let mut ai = Rng::from_seed(seed ^ 0xA1);
    let driver = Seat::A;
    let stdin = io::stdin();
    loop {
        // Let the AI take Seat B's turns until it's the driver's move or the end.
        loop {
            if matches!(engine.state().phase, Phase::Finished { .. }) {
                emit_result(seed, &engine.state().phase);
                return;
            }
            if engine.state().active == driver {
                break;
            }
            let cmd = recollect_bot::choose(&engine, engine.state().active, difficulty, &mut ai);
            let _ = engine.apply(engine.state().active, cmd);
        }
        // Emit the driver's redacted view, then read one command.
        println!(
            "{}",
            serde_json::to_string(&view_for(&engine, driver)).unwrap()
        );
        io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            return; // EOF — the driver disconnected
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Command>(trimmed) {
            Ok(cmd) => {
                if let Err(r) = engine.apply(driver, cmd) {
                    println!("{}", serde_json::json!({"rejected": format!("{r:?}")}));
                }
            }
            Err(e) => println!(
                "{}",
                serde_json::json!({"error": format!("bad command: {e}")})
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoplay_reaches_a_result_deterministically() {
        // A both-AI game must terminate (the 12-round clock) — drive it headless
        // and confirm it finishes. (Output goes to stdout; we just assert no panic
        // and a finished phase via a direct engine run mirroring autoplay.)
        let (mut engine, _) = new_engine(7);
        let mut ai = Rng::from_seed(7 ^ 0xA1);
        let mut steps = 0;
        while !matches!(engine.state().phase, Phase::Finished { .. }) {
            let seat = engine.state().active;
            let cmd = recollect_bot::choose(&engine, seat, Difficulty::Normal, &mut ai);
            engine.apply(seat, cmd).expect("bot move is legal");
            steps += 1;
            assert!(steps < 5000, "a headless AI match must terminate");
        }
    }

    /// The headless JSON protocol drives Seat A by deserializing one `Command` per
    /// stdin line and `apply`ing it (see [`protocol`]). This guards the exact wire
    /// forms the §5 opening uses: the Mulligan `{"Mulligan":{"seat":"A"}}` and the
    /// Glimpse `{"Choose":{"index":N}}` parse to the right `Command` AND are ACCEPTED
    /// on the opener's view (round 1, Seat A). `new_engine` opens Seat A, so the
    /// driver's first view is its own — the same state a real `headless` session reads.
    #[test]
    fn protocol_accepts_mulligan_and_glimpse_choose_json_on_the_openers_view() {
        let driver = Seat::A;

        // Mulligan — `{"Mulligan":{"seat":"A"}}` decodes to the seat-stamped command
        // and is legal in the opening window (round 1, the driver untouched).
        let (mut engine, _) = new_engine(6);
        assert_eq!(
            engine.state().active,
            driver,
            "the driver opens (its own view)"
        );
        let mull: Command =
            serde_json::from_str(r#"{"Mulligan":{"seat":"A"}}"#).expect("Mulligan JSON parses");
        assert_eq!(mull, Command::Mulligan { seat: Seat::A });
        engine
            .apply(driver, mull)
            .expect("the opener's Mulligan is accepted on its view");

        // Glimpse — open the §5 Glimpse, then drive its two steps purely as
        // `{"Choose":{"index":N}}` JSON lines: index 0 BURNS a hand card (step 1),
        // index 1 BOTTOMS the peeked top for +1 anima (step 2). Both accepted.
        let (mut engine, _) = new_engine(6);
        engine
            .apply(driver, Command::Glimpse)
            .expect("Glimpse is legal at the opening");
        let burn: Command =
            serde_json::from_str(r#"{"Choose":{"index":0}}"#).expect("Choose JSON parses");
        assert_eq!(burn, Command::Choose { index: 0 });
        engine
            .apply(driver, burn)
            .expect("the Glimpse BURN choice is accepted as JSON");
        let bottom: Command =
            serde_json::from_str(r#"{"Choose":{"index":1}}"#).expect("Choose JSON parses");
        engine
            .apply(driver, bottom)
            .expect("the Glimpse keep/bottom choice is accepted as JSON");
    }
}
