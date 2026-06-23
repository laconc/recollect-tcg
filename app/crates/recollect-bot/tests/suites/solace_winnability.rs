//! Gate: the Solace PvE fight must stay WINNABLE and not a pushover. If a future
//! change to scoring or the bot eval pushes the player win rate out of a fair
//! band, this fails — catching both "the Solace became unbeatable" and "the
//! Solace is a pushover" regressions. The Solace plays to win: it persists,
//! fights, and scores by standing Unwritten + its off-board erasure tally.
//!
//! IMPORTANT — the match is built with the REAL factions (`[Lorekeeper, Solace]`), so seat B's
//! asymmetric scoring is in force: its Unwritten leave no impression, the Dusk sweeps them, and each
//! banish/unwrite banks the erasure tally that joins B's score at Nightfall. A plain `Engine::new`
//! defaults both seats to Lorekeeper, which would silently make seat B score like a Lorekeeper
//! (stamping board impressions that survive the Dusk) and measure a mirror, not the PvE fight.
//! Building with the real factions is what keeps this gate measuring the asymmetric PvE contest.
//! Here we assert the **Hard** mirror is fair (0.25–0.86) — the mid-skill
//! contest, neither trivial nor lost. Hard is a hotter **depth-2** tier (the depth split is
//! Easy/Normal depth-1, Hard/Expert depth-2), and depth-2 is exactly where the Solace's denial game
//! earns its keep — so the per-character `char_sweep` measure runs **~46% player-win at the Hard
//! mirror** (a near-even fight: the board-scoring Lorekeeper's structural edge against the Solace's
//! depth-2 walling), near the centre of this gate's band, not its edge.
//! (At the **Expert** mirror `char_sweep` player-win sits ~51% — both depth-2 mirrors land near even,
//! and the tiers separate by temperature in Quick Play — see the `char_sweep` / `calibrate` Bal2
//! re-sweep notes and `docs/difficulty.md`.)
use recollect_bot::{Difficulty, Faction, choose_as};
use recollect_core::cards::canon_catalog;
use recollect_core::quickplay::{generate_deck, generate_deck_for};
use recollect_core::rng::Rng;
use recollect_core::state::{MatchResult, MatchRules, Phase};
use recollect_core::types::{CardDef, CardId};
use recollect_core::{Engine, Seat};

/// Build a 1v1 PvE engine with the REAL factions in force: seat A Lorekeeper, seat B the Solace.
/// This matters — a plain `Engine::new` defaults both seats to Lorekeeper, which silently makes seat
/// B score like a Lorekeeper (stamping board impressions that survive the Dusk) instead of the Solace
/// (no impression, swept by the Dusk, banking the off-board erasure tally). Without this the "Solace
/// fight" is a Lorekeeper mirror with Solace cards, not the asymmetric PvE contest.
fn pve_engine(seed: u64, cat: &[CardDef], da: Vec<CardId>, db: Vec<CardId>) -> Engine {
    let mut rules = MatchRules::default();
    rules.factions = [Faction::Lorekeeper, Faction::Solace];
    Engine::new_with_rules(seed, cat.to_vec(), da, db, rules, Seat::A).0
}

#[test]
fn the_solace_pve_fight_is_winnable_and_fair() {
    let cat = canon_catalog();
    let n = 150u64;
    let mut player_wins = 0u64;
    for seed in 0..n {
        // A real PvE telling — the player (A) fields a Lorekeeper deck, the Solace (B) a
        // faction-pure Solace deck; both piloted by the faction-aware bot.
        let da = generate_deck((seed % 6) as u8, seed, &cat);
        let db = generate_deck_for(
            recollect_core::types::Faction::Solace,
            ((seed + 2) % 6) as u8,
            seed ^ 0x5EED,
            &cat,
        );
        let mut e = pve_engine(seed, &cat, da, db);
        let mut rng = Rng::from_seed(seed ^ 0xA);
        let mut steps = 0;
        loop {
            if let Phase::Finished { result, .. } = e.state().phase {
                if matches!(result, MatchResult::Win(Seat::A)) {
                    player_wins += 1;
                }
                break;
            }
            if steps > 5000 {
                break;
            }
            let seat = e.state().active;
            let cmd = if seat == Seat::B {
                choose_as(
                    &e,
                    seat,
                    Difficulty::Hard,
                    Faction::Solace,
                    Faction::Lorekeeper,
                    &mut rng,
                )
            } else {
                choose_as(
                    &e,
                    seat,
                    Difficulty::Hard,
                    Faction::Lorekeeper,
                    Faction::Solace,
                    &mut rng,
                )
            };
            e.apply(seat, cmd.clone())
                .unwrap_or_else(|err| panic!("seat {seat:?} cmd {cmd:?} rejected: {err:?}"));
            steps += 1;
        }
    }
    let rate = player_wins as f64 / n as f64;
    // Fair band: the player must win a real share (not unwinnable) but the Solace must remain a
    // threat (not a pushover). Wide band — this guards against catastrophic regressions, not fine
    // balance (that's bin/char_sweep). With the REAL Solace economy in force (factions set above) the
    // Hard mirror sits ~45% player-win on these generated decks (the per-character `char_sweep` roster
    // reads ~46%): a near-even fight where the board-scoring Lorekeeper's structural edge meets the
    // Solace's depth-2 walling — comfortably inside the band, near its centre. The lower bound (0.25)
    // guards "unwinnable for the player"; the upper (0.86) "pushover Solace".
    assert!(
        rate > 0.25,
        "the Solace looks unwinnable: player won only {:.0}%",
        rate * 100.0
    );
    assert!(
        rate < 0.86,
        "the Solace looks like a pushover: player won {:.0}%",
        rate * 100.0
    );
}

/// Red-team (beyond the golden path): the Solace must be an ACTIVE opponent. The win-rate
/// test above checks the *outcome*; this checks the *mechanism* — across many character matches
/// the Solace should play creatures, strike (the `engage` arrival), and tell Unwriting events,
/// not pass its turns away. If a regression made it win by some degenerate non-playing path, or
/// stop using the new `TellUnwriting` seam, this catches it where a win-rate band would not.
#[test]
fn the_solace_actively_plays_its_deck() {
    use recollect_core::quickplay::solace_character_deck;
    use recollect_core::state::Command;
    let cat = canon_catalog();
    let n = 40u64;
    let (mut plays, mut engages, mut tells) = (0u64, 0u64, 0u64);
    for seed in 0..n {
        let da = generate_deck((seed % 6) as u8, seed, &cat);
        let db = solace_character_deck((seed % 20) as usize, seed ^ 0x5EED, &cat);
        let mut e = pve_engine(seed, &cat, da, db);
        let mut rng = Rng::from_seed(seed ^ 0xB);
        let mut steps = 0;
        loop {
            if matches!(e.state().phase, Phase::Finished { .. }) || steps > 5000 {
                break;
            }
            let seat = e.state().active;
            let cmd = if seat == Seat::B {
                choose_as(
                    &e,
                    seat,
                    Difficulty::Hard,
                    Faction::Solace,
                    Faction::Lorekeeper,
                    &mut rng,
                )
            } else {
                choose_as(
                    &e,
                    seat,
                    Difficulty::Hard,
                    Faction::Lorekeeper,
                    Faction::Solace,
                    &mut rng,
                )
            };
            if seat == Seat::B {
                match &cmd {
                    Command::PlaySpirit { engage, .. } => {
                        plays += 1;
                        if engage.is_some() {
                            engages += 1;
                        }
                    }
                    Command::MoveSpirit {
                        engage: Some(_), ..
                    } => engages += 1,
                    Command::TellUnwriting { .. } => tells += 1,
                    _ => {}
                }
            }
            e.apply(seat, cmd).expect("legal");
            steps += 1;
        }
    }
    assert!(
        plays > n,
        "the Solace barely played creatures ({plays} over {n} matches)"
    );
    assert!(
        engages > 0,
        "the Solace never struck — no combat across {n} matches"
    );
    assert!(
        tells > 0,
        "the Solace never used the TellUnwriting seam across {n} matches"
    );
}
