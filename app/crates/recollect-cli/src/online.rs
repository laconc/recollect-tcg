//! Networked transport: the authoritative server is the only authority — this
//! renders `PlayerView`s and forwards verbs over the versioned ws protocol.
//! Two interfaces share one socket loop: the interactive TUI (verbs in,
//! [`render_view`] out) and a `--json` machine mode (one `Command` JSON per
//! stdin line, the resulting view/result as JSON on stdout).
use crate::render::{render_team, render_view};
use futures_util::{SinkExt, StreamExt};
use recollect_core::cards::canon_catalog;
use recollect_core::state::{Command, Phase};
use recollect_core::types::CardDef;
use recollect_protocol::{ClientMsg, PROTOCOL_VERSION, ServerMsg};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_tungstenite::tungstenite::Message;

/// What to do on connect: create a fresh match (claim seat A) or join one.
/// `New.query` is appended to `POST /matches` (e.g. "?opponent=bot") so the
/// server can fill Seat B with its AI.
pub enum Action {
    New { query: String },
    Join { match_id: String, token: String },
}

/// Parse + resolve one verb line against the server's legal menu. The verb
/// grammar lives in the shared [`crate::verbs`] module — the SAME parser local play
/// uses, so both human modes speak one language. Tiles take board indices (`12`) or
/// grid coordinates (`c3`); `bw` is the board width (5 for 1v1, 6 for 2v2).
/// `Evolve`/`Devolve`/`Reclaim` name a tile (and `Mulligan` names no seat) and resolve
/// to the matching legal `Command`.
fn parse_verb(line: &str, legal: &[recollect_protocol::LegalMove], bw: u8) -> Option<Command> {
    let intent = crate::verbs::parse(line, bw)?;
    let cmds: Vec<Command> = legal.iter().map(|m| m.cmd.clone()).collect();
    crate::verbs::resolve(intent, &cmds)
}

pub async fn run(server: String, action: Action, json: bool) {
    let (match_id, token) = match action {
        Action::New { query } => {
            let body = ureq_post(&format!("{server}/matches{query}")).await;
            // Setup metadata to stderr, so `--json` keeps a clean JSON stdout.
            eprintln!("match {} created.", body["match_id"]);
            // 2v2 hands back four slot tokens; we take A1 and print the rest to share.
            if let Some(slots) = body["slot_tokens"].as_object() {
                for k in ["B1", "A2", "B2"] {
                    if let Some(t) = slots.get(k).and_then(|v| v.as_str()) {
                        eprintln!("  slot {k} token (hand to a player): {t}");
                    }
                }
                eprintln!("  joining as slot A1…");
                (
                    body["match_id"].to_string(),
                    slots["A1"].as_str().unwrap().to_string(),
                )
            } else {
                match body["seat_b_token"].as_str() {
                    Some(tok) => eprintln!("  seat B token (hand to your opponent): {tok}"),
                    None => eprintln!("  opponent: the server's bot (you are seat A)"),
                }
                eprintln!("  joining as seat A…");
                (
                    body["match_id"].to_string(),
                    body["seat_a_token"].as_str().unwrap().to_string(),
                )
            }
        }
        Action::Join { match_id, token } => (match_id, token),
    };
    let ws_url = format!(
        "{}/matches/{}/ws",
        server.replacen("http", "ws", 1),
        match_id
    );
    let cat = canon_catalog();
    // The client sequence persists across reconnects (it only ever increases, so a
    // command after a resume is never seen as stale by the server).
    let mut seq: u64 = 0;
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    if !json {
        eprintln!(
            "controls: a move's number · verbs p/o/m/v/dv/rc/g/r/end (tiles as 0-24 or c3) · q to quit (auto-reconnects if dropped)"
        );
    }
    // Auto-reconnect: a dropped socket re-Hellos the same match (the server keeps
    // it in memory, or rebuilds a journaled one); only a clean quit or a finished
    // match ends the loop. Connect failures back off and eventually give up.
    let mut connect_fails = 0u32;
    loop {
        match play_socket(&ws_url, &token, json, &cat, &mut seq, &mut lines).await {
            Outcome::Quit | Outcome::Finished => break,
            Outcome::Dropped => {
                connect_fails = 0;
                if !json {
                    eprintln!("— disconnected; reconnecting… —");
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Outcome::ConnectFailed => {
                connect_fails += 1;
                if connect_fails > 10 {
                    eprintln!("could not reach {ws_url} after {connect_fails} tries; giving up");
                    break;
                }
                let ms = 250u64 * (1u64 << connect_fails.min(5));
                if !json {
                    eprintln!("— connect failed (attempt {connect_fails}); retrying in {ms}ms —");
                }
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
        }
    }
}

/// Why a single connection ended — drives the reconnect loop in [`run`].
enum Outcome {
    /// The user typed `q` or stdin closed.
    Quit,
    /// The match reached a result; nothing to reconnect to.
    Finished,
    /// The socket dropped mid-game; reconnect and resume.
    Dropped,
    /// Couldn't establish the socket (server down / match gone); back off.
    ConnectFailed,
}

/// One connection's lifetime: connect, `Hello`, then pump the server's views and
/// the user's commands until the socket drops, the user quits, or the game ends.
async fn play_socket<R: tokio::io::AsyncBufRead + Unpin>(
    ws_url: &str,
    token: &str,
    json: bool,
    cat: &[CardDef],
    seq: &mut u64,
    lines: &mut tokio::io::Lines<R>,
) -> Outcome {
    let Ok((ws, _)) = tokio_tungstenite::connect_async(ws_url).await else {
        return Outcome::ConnectFailed;
    };
    let (mut tx, mut rx) = ws.split();
    // The token rides the first frame, never the URL.
    let hello = ClientMsg::Hello {
        v: PROTOCOL_VERSION,
        match_token: token.to_string(),
        name: None,
        session_id: None,
    };
    if tx
        .send(Message::Text(serde_json::to_string(&hello).unwrap().into()))
        .await
        .is_err()
    {
        return Outcome::ConnectFailed;
    }
    let mut legal: Vec<recollect_protocol::LegalMove> = Vec::new();
    // Board width for the verb parser's grid-coordinate tiles (5 for 1v1, 6 for 2v2);
    // refreshed from each view's tile count.
    let mut bw: u8 = 5;
    loop {
        tokio::select! {
            msg = rx.next() => {
                let Some(Ok(Message::Text(txt))) = msg else { return Outcome::Dropped; };
                match serde_json::from_str::<ServerMsg>(&txt) {
                    Ok(ServerMsg::Welcome { view, legal: moves, .. })
                    | Ok(ServerMsg::Applied { view, legal: moves, .. })
                    | Ok(ServerMsg::Update { view, legal: moves, .. }) => {
                        legal = moves;
                        bw = (view.tiles.len() as f64).sqrt().round() as u8;
                        let done = matches!(view.phase, Phase::Finished { .. });
                        if json {
                            println!("{}", serde_json::json!({ "view": view, "legal": legal }));
                        } else {
                            print!("{}", render_view(&view, cat));
                            for (i, m) in legal.iter().enumerate() {
                                println!("  {i:>3}. {}", m.label);
                            }
                        }
                        if done { return Outcome::Finished; }
                    }
                    Ok(ServerMsg::Rejected { reason, seq: rs, .. }) => {
                        if json {
                            println!("{}", serde_json::json!({"rejected": reason, "seq": rs}));
                        } else {
                            println!("rejected: {reason}");
                        }
                    }
                    Ok(ServerMsg::Error { message, .. }) => {
                        if json { println!("{}", serde_json::json!({"error": message})); }
                        else { println!("error: {message}"); }
                    }
                    Ok(ServerMsg::Pong { .. }) => {}
                    // 2v2: a slot's TeamView on the 6×6 board, with the same legal menu.
                    Ok(ServerMsg::TeamWelcome { view, legal: moves, .. })
                    | Ok(ServerMsg::TeamApplied { view, legal: moves, .. })
                    | Ok(ServerMsg::TeamUpdate { view, legal: moves, .. }) => {
                        legal = moves;
                        bw = view.board_w.max(1) as u8;
                        let done = matches!(view.phase, Phase::Finished { .. });
                        if json {
                            println!("{}", serde_json::json!({ "team_view": view, "legal": legal }));
                        } else {
                            render_team(&view, cat);
                            for (i, m) in legal.iter().enumerate() {
                                println!("  {i:>3}. {}", m.label);
                            }
                        }
                        if done { return Outcome::Finished; }
                    }
                    // End-of-match seed reveal (provably-fair shuffle); accepted and not surfaced.
                    Ok(ServerMsg::SeedRevealed { .. }) => {}
                    Err(e) => eprintln!("unreadable server message: {e}"),
                }
            }
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { return Outcome::Quit; };
                let trimmed = line.trim();
                if trimmed == "q" { return Outcome::Quit; }
                let command = if json {
                    match serde_json::from_str::<Command>(trimmed) {
                        Ok(c) => Some(c),
                        Err(e) => { eprintln!("bad command JSON: {e}"); None }
                    }
                } else if let Ok(n) = trimmed.parse::<usize>() {
                    // A number picks from the server's legal-move menu.
                    match legal.get(n) {
                        Some(m) => Some(m.cmd.clone()),
                        None => { eprintln!("no move {n} (0-{})", legal.len().saturating_sub(1)); None }
                    }
                } else {
                    match parse_verb(trimmed, &legal, bw) {
                        Some(c) => Some(c),
                        None => {
                            eprintln!("{}", crate::verbs::USAGE);
                            None
                        }
                    }
                };
                if let Some(command) = command {
                    *seq += 1;
                    let msg = ClientMsg::Cmd { v: PROTOCOL_VERSION, seq: *seq, command };
                    if tx.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await.is_err() {
                        return Outcome::Dropped;
                    }
                }
            }
        }
    }
}

/// Tiny POST helper over std (no extra HTTP dep): http://host:port/path only.
async fn ureq_post(url: &str) -> serde_json::Value {
    let stripped = url.strip_prefix("http://").expect("local http URL");
    let (host, path) = stripped.split_once('/').expect("path");
    let req = format!(
        "POST /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let mut stream = tokio::net::TcpStream::connect(host)
        .await
        .expect("server reachable");
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("{}");
    serde_json::from_str(body.trim()).expect("json body")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_parse_to_commands() {
        assert_eq!(
            parse_verb("p 0 12 e 13", &[], 5),
            Some(Command::PlaySpirit {
                hand_index: 0,
                tile: 12,
                engage: Some(13),
                chain_prefs: Vec::new()
            })
        );
        assert_eq!(
            parse_verb("o 1 7", &[], 5),
            Some(Command::Overwrite {
                hand_index: 1,
                tile: 7
            })
        );
        assert_eq!(
            parse_verb("m 6 7", &[], 5),
            Some(Command::MoveSpirit {
                from: 6,
                to: 7,
                engage: None
            })
        );
        assert_eq!(parse_verb("end", &[], 5), Some(Command::EndTurn));
        assert_eq!(parse_verb("dance", &[], 5), None);
    }
}
