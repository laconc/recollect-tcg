//! End-to-end: the real `recollect online --json` binary plays against a live
//! (in-process) server over a real WebSocket. Proves the CLI's networked-JSON
//! glue — create the match, connect, receive the seat's `PlayerView` as JSON,
//! and forward a `Command` JSON from stdin — round-trips against the authority.
//!
//! In-memory server (no Postgres), so this runs in the normal suite.
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

/// Next JSON object the client prints, skipping blanks/non-view lines, bounded by
/// a timeout so a hang fails loudly instead of stalling the suite.
async fn next_view<R: AsyncBufRead + Unpin>(lines: &mut Lines<R>) -> serde_json::Value {
    loop {
        let line = tokio::time::timeout(Duration::from_secs(20), lines.next_line())
            .await
            .expect("client produced output in time")
            .expect("client stdout is readable");
        let Some(line) = line else {
            panic!("client closed stdout before emitting a view");
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Online JSON frames are `{"view": <PlayerView>, "legal": [...]}`.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed)
            && let Some(view) = v.get("view")
            && view.get("seat").is_some()
        {
            return view.clone();
        }
    }
}

/// First-player: the first seed that opens A even under a strong B-initiative bias (−40), so a
/// vs-bot character's initiative can't flip it. Passed to the binary as `--seed`, giving these wire
/// tests (each drives only seat A) a deterministic A-opener — no global env, no unsafe.
fn a_opener_seed() -> String {
    (0u64..)
        .find(|&s| recollect_core::quickplay::decide_opener(s, -40) == recollect_core::Seat::A)
        .unwrap()
        .to_string()
}

#[ignore = "slow: spins up a live server over a real socket; run via `make test-slow` or nightly"]
#[tokio::test]
async fn online_json_round_trips_a_view_against_a_live_server() {
    // A live, in-memory server on an ephemeral port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(recollect_server::serve_in_memory(listener));

    // The real `recollect` binary as the JSON client: create a match (pinned A-opener), claim seat A.
    let seed = a_opener_seed();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_recollect"))
        .args([
            "online",
            "new",
            "--seed",
            &seed,
            "--json",
            "--server",
            &format!("http://{addr}"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the recollect binary");
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut stdin = child.stdin.take().unwrap();

    // The preamble: Seat A's own redacted view, as JSON, straight off the wire.
    let welcome = next_view(&mut out).await;
    assert_eq!(welcome["seat"], "A");
    assert_eq!(welcome["active"], "A", "Seat A opens");

    // Drive one command as JSON on stdin; the server applies it and the next view
    // shows the turn passed to B — a full client→server→client JSON round-trip.
    stdin.write_all(b"\"EndTurn\"\n").await.unwrap();
    stdin.flush().await.unwrap();
    let applied = next_view(&mut out).await;
    assert_eq!(
        applied["seat"], "A",
        "still only our own seat's view (redaction)"
    );
    assert_eq!(
        applied["active"], "B",
        "EndTurn passed the turn over the wire"
    );

    drop(stdin);
    let _ = child.kill().await;
}

/// `online new --vs-bot`: the flag asks the server to fill Seat B with its AI.
/// After we pass, the server drives the bot's whole turn, so the next view is
/// ours again — proving the flag plumbs through to a server-driven opponent.
#[ignore = "slow: spins up a live server over a real socket; run via `make test-slow` or nightly"]
#[tokio::test]
async fn vs_bot_flag_plays_the_server_bot() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(recollect_server::serve_in_memory(listener));

    let seed = a_opener_seed();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_recollect"))
        .args([
            "online",
            "new",
            "--vs-bot",
            "--seed",
            &seed,
            "--json",
            "--server",
            &format!("http://{addr}"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the recollect binary");
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut stdin = child.stdin.take().unwrap();

    let welcome = next_view(&mut out).await;
    assert_eq!(welcome["seat"], "A");
    assert_eq!(welcome["active"], "A", "Seat A opens");

    // Pass our turn; the server's bot takes Seat B's whole turn, so it is our
    // move again when the ack comes back.
    stdin.write_all(b"\"EndTurn\"\n").await.unwrap();
    stdin.flush().await.unwrap();
    let after = next_view(&mut out).await;
    assert_eq!(after["seat"], "A");
    assert_eq!(
        after["active"], "A",
        "the server bot took Seat B's turn; it is ours again"
    );

    drop(stdin);
    let _ = child.kill().await;
}
