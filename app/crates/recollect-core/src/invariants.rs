//! The state-validity invariants — the single source of truth for "a `GameState`
//! that the engine produced is well-formed." One definition, consumed by both the
//! sampling fuzz (the full-catalog playthrough `tests/suites/fuzz.rs`,
//! checked after every command), the proptest properties (`tests/suites/props.rs`), and the exhaustive
//! stateright model-check (`recollect-verify`, checked on every reachable state),
//! so the two can never drift apart. Reachability/liveness (does a legal command
//! exist?) is NOT here — that needs the engine, not just the state, and lives with
//! its checker. See `docs/testing.md` "Invariants registry".
use crate::state::{GameState, Phase};
use crate::types::Seat;

/// Returns `Err(reason)` naming the first violated invariant, else `Ok(())`.
pub fn check(st: &GameState) -> Result<(), String> {
    let board = &st.board;
    let mut score_a = 0usize;
    let mut score_b = 0usize;
    for (i, t) in board.iter().enumerate() {
        // 1) A tile never holds BOTH a spirit and terrain.
        if t.spirit.is_some() && t.terrain.is_some() {
            return Err(format!("tile {i}: spirit AND terrain coexist"));
        }
        // 1b) A Stray stands on its tile (it occupies, §2/§6), so no spirit may share it
        // either — a spirit AND a Stray on one tile is as illegal as spirit + terrain.
        // (A spirit is placed only onto an empty tile; an Overwrite onto a Stray clears the
        // wild before the overwriter lands.) Guards the play/overwrite-onto-Stray paths.
        if t.spirit.is_some()
            && st
                .stray
                .as_ref()
                .map(|s| s.tile == i as u8)
                .unwrap_or(false)
        {
            return Err(format!("tile {i}: spirit AND stray coexist"));
        }
        // 1c) A Stray also occupies, so no TERRAIN may share its tile either — terrain AND a
        // Stray on one tile is as illegal as spirit + Stray. (The landmark/fabrication
        // placement guards reject terrain onto a Stray's tile; this catches the class
        // directly, the way 1b catches spirit + Stray. Without it a Landmark dropped on a
        // Stray's tile, then Overwritten, produced the #1 spirit+terrain violation only AFTER
        // the wild was cleared — a step removed from the real cause.)
        if t.terrain.is_some()
            && st
                .stray
                .as_ref()
                .map(|s| s.tile == i as u8)
                .unwrap_or(false)
        {
            return Err(format!("tile {i}: terrain AND stray coexist"));
        }
        // 2) No terrain ever sits on a faded (Dusk-contracted) tile.
        if t.faded && t.terrain.is_some() {
            return Err(format!("tile {i}: terrain on a faded tile"));
        }
        if let Some(sp) = &t.spirit {
            // 3) HP bounded: never above max; a standing (non-fading) spirit is positive.
            if sp.hp > sp.hp_max {
                return Err(format!(
                    "tile {i}: hp {} > hp_max {} (overheal)",
                    sp.hp, sp.hp_max
                ));
            }
            if !sp.fading && sp.hp <= 0 {
                return Err(format!("tile {i}: a standing spirit has hp {} <= 0", sp.hp));
            }
            // 4) Stats never go wildly negative (underflow / runaway debuff).
            if sp.attack < -100 || sp.defense < -100 {
                return Err(format!(
                    "tile {i}: stat underflow a={} d={}",
                    sp.attack, sp.defense
                ));
            }
            // 5) A *standing* token never records a banisher. Tokens leave no impression,
            // so a STANDING (non-fading) token with a banisher would be a stale combat
            // record leaked onto a live body (the bug class this guards). The one
            // legitimate carrier is a *fading* UNWRITTEN token inside its standing-Faded
            // window — an Unwritten base banished in combat lingers Faded so the Solace can
            // **Primal-Deepen** it (§5), and the window machinery (5b) needs `banished_by`
            // set; the Unwritten's own dissolve still leaves nothing, so no impression ever
            // falls. (A defeated KINDRED token never reaches this state at all — combat
            // dissolves it at once, leaving no mark; see `banish_or_replace`.) Restricting
            // the check to standing tokens keeps it true without a catalog here.
            if sp.is_token && !sp.fading && sp.banished_by.is_some() {
                return Err(format!(
                    "tile {i}: a standing token recorded a banisher (tokens leave no impression)"
                ));
            }
            // 5b) The standing-Faded window is the ONLY way a spirit is Fading. Since the
            // Dusk is now instant (it dissolves its rim Unwritten at the contraction,
            // never deferring a fade) and the Fade phase moved to turn-END, every Fading
            // spirit is a COMBAT-banished base inside its window — so the implication runs
            // BOTH ways: `fading <=> fade_deadline.is_some()` (and a deadline ⇒ a recorded
            // `banished_by`, since it is combat-only, on a reachable round `<= last_round +
            // 1`, the finish guard). A fading spirit WITHOUT a deadline would mean a fade
            // is waiting for a step that no longer exists (a leaked uncontested/Dusk fade);
            // a standing spirit WITH a deadline would mean the linger leaked onto a body
            // that already redeemed.
            if sp.fading != sp.fade_deadline.is_some() {
                return Err(format!(
                    "tile {i}: fading={} but fade_deadline.is_some()={} — every Fading spirit \
                     must carry a deadline now that the Dusk is instant and Fade is at turn-END",
                    sp.fading,
                    sp.fade_deadline.is_some()
                ));
            }
            if let Some(deadline) = sp.fade_deadline {
                if sp.banished_by.is_none() {
                    return Err(format!(
                        "tile {i}: a fade_deadline without a banisher (the window is combat-only)"
                    ));
                }
                if deadline > st.rules.last_round + 1 {
                    return Err(format!(
                        "tile {i}: fade_deadline {deadline} past last_round {} + 1",
                        st.rules.last_round
                    ));
                }
            }
            if !sp.fading {
                match sp.owner {
                    Seat::A => score_a += 1,
                    Seat::B => score_b += 1,
                }
            }
        }
    }
    // 6) Standing spirits can never exceed the board's tile count.
    if score_a + score_b > board.len() {
        return Err(format!(
            "standing spirits {score_a}+{score_b} exceed board {}",
            board.len()
        ));
    }
    // 7) A finished match's recorded scores obey the same bound.
    if let Phase::Finished {
        score_a, score_b, ..
    } = st.phase
        && score_a as usize + score_b as usize > board.len()
    {
        return Err(format!(
            "final score {score_a}+{score_b} exceeds board {}",
            board.len()
        ));
    }
    // 8) The round never runs past the rules' last round + 1 (the finish guard).
    if st.round > st.rules.last_round + 1 {
        return Err(format!(
            "round {} ran past last_round {}",
            st.round, st.rules.last_round
        ));
    }
    Ok(())
}
