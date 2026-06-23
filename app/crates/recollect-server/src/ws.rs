//! WebSocket connection handling: the per-seat socket loop + Hello routing.
//! A sibling of `server/lib.rs`; `use super::*` shares AppState/Match + imports.
//!
//! The loop is a thin pump between one socket and the match **actor**.
//! On Hello it routes the token to a [`Principal`], `subscribe`s to the actor
//! (receiving a per-seat mpsc receiver + the welcome frame), then selects between
//! actor-pushed frames (fanned out per-seat — no lossy broadcast) and incoming
//! socket text (forwarded to the actor, whose reply is the seat's ack). All engine
//! state and the journaled `.await` live in the actor; the transport holds no lock.
use super::*;
use tokio::sync::mpsc;

#[tracing::instrument(skip_all, fields(match_id = id))]
pub(crate) async fn ws_handler(
    State(app): State<AppState>,
    Path(id): Path<u64>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let entry = { lock_matches(&app.matches).get(&id).cloned() };
    // A miss may be a journaled match the server has forgotten (a restart, or
    // a sweep) — try to rebuild it from the journal before giving up.
    let entry = match entry {
        Some(e) => e,
        None => match recover_match(&app, id).await {
            Some(e) => e,
            None => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    // The seat token arrives in the FIRST FRAME (ClientMsg::Hello), never in
    // the URL — proxies log nothing usable.
    ws.max_message_size(16 * 1024)
        .on_upgrade(move |mut socket| async move {
            let hello =
                tokio::time::timeout(std::time::Duration::from_secs(10), socket.recv()).await;
            let Ok(Some(Ok(Message::Text(txt)))) = hello else {
                return;
            };
            let Ok(ClientMsg::Hello {
                match_token,
                name,
                session_id,
                ..
            }) = serde_json::from_str::<ClientMsg>(&txt)
            else {
                crate::metrics::frame_rejected(crate::metrics::RejectReason::HelloFirst);
                let _ = socket
                    .send(Message::Text(
                        serde_json::to_string(&ServerMsg::Error {
                            v: PROTOCOL_VERSION,
                            message: "hello_first".into(),
                        })
                        .unwrap()
                        .into(),
                    ))
                    .await;
                return;
            };
            // Tokens are stored hashed — authorise by hashing the presented
            // plaintext and comparing (constant time), never a clear-text compare.
            let routed = match &entry.kind {
                MatchKind::OneVsOne { token_a, token_b } => {
                    if token_a.matches(&match_token) {
                        Some(Routed::Seat(Seat::A))
                    } else if token_b.matches(&match_token) {
                        Some(Routed::Seat(Seat::B))
                    } else {
                        None
                    }
                }
                MatchKind::TwoVsTwo { tokens } => tokens
                    .iter()
                    .find(|(_, t)| t.matches(&match_token))
                    .map(|(s, _)| Routed::Slot(*s)),
            };
            // An unroutable token is a stranger: refuse it before any record —
            // a `bad_token` connection never occupied a seat, so it's neither a join
            // nor a participant.
            let Some(routed) = routed else {
                crate::metrics::frame_rejected(crate::metrics::RejectReason::BadToken);
                let _ = socket
                    .send(Message::Text(
                        serde_json::to_string(&ServerMsg::Error {
                            v: PROTOCOL_VERSION,
                            message: "bad_token".into(),
                        })
                        .unwrap()
                        .into(),
                    ))
                    .await;
                return;
            };
            // Best-effort journal records for an authorised join — spawned, and only
            // when a journal is attached (never blocks or fails the connection):
            //   • the anonymous-session join (usage_events), keyed by session_id;
            //   • the name-tagged participant (match_participants) — the handle
            //     the seat connected under (a server fallback if the client sent
            //     none), so EVERY journaled match records who played, anon and
            //     signed-in alike. The client owns minting/persisting the real handle
            //     (the web lane); the server records it + supplies the floor.
            if let Some(j) = entry.journal.clone() {
                let db_id = entry.db_id.clone();
                let seat_label = routed.seat_label();
                let sid = session_id
                    .clone()
                    .map(|s| s.chars().take(64).collect::<String>()); // cap untrusted id
                let handle = crate::identity::participant_handle(name.as_deref());
                tokio::spawn(async move {
                    let j = j.lock().await;
                    if let Err(e) = j.record_usage(sid.as_deref(), "joined", Some(&db_id)).await {
                        tracing::warn!(error = %e, "usage_events record failed");
                    }
                    if let Err(e) = j
                        .record_participant(&db_id, seat_label, &handle, sid.as_deref(), None)
                        .await
                    {
                        tracing::warn!(error = %e, "match_participants record failed");
                    }
                });
            }
            let who = match routed {
                Routed::Seat(seat) => Principal::Seat(seat),
                Routed::Slot(slot) => Principal::Slot(slot),
            };
            // The display name rides the Subscribe into the actor (1v1 only); it
            // is ignored for slots, so pass it through uniformly.
            socket_loop(socket, entry, who, name).await;
        })
        .into_response()
}

/// The rate-limit window in seconds: a bucket older than this resets, and is
/// eligible for eviction (see [`rate_ok`]). One minute — matches the prose in the
/// account/match handlers and the §9 threat-model row.
const RATE_WINDOW_SECS: u64 = 60;

/// Once the rate map holds this many IPs, an insert first sweeps every bucket whose
/// window has fully expired. The cap is generous (a real playtest box sees far fewer
/// concurrent IPs than this), so the sweep is rare; it exists only to bound the map
/// against an enumeration flood (a botnet rotating source IPs would otherwise grow
/// it one entry per IP forever — the buckets RESET on expiry but are never EVICTED
/// otherwise).
const RATE_MAP_SWEEP_AT: usize = 10_000;

/// A small per-IP token bucket for the HTTP surface, with two properties:
///   1. **Bounded memory.** A distinct source IP otherwise leaves a permanent entry
///      (the window resets in place but the key is never removed), so an attacker
///      rotating IPs would grow the map without limit — a slow unbounded-memory DoS.
///      When the map reaches [`RATE_MAP_SWEEP_AT`] an insert first drops every
///      fully-expired bucket, so the live size tracks *active* IPs, not all-time ones.
///   2. **Poison-resilient.** A `Mutex` poisoned by a panic elsewhere must not turn
///      every future rate check into a panic (a server-wide cascade from one unlucky
///      thread). Recover the guard from the poison rather than `unwrap()`-ing: the
///      bucket data is a plain counter, safe to keep using after a panic that never
///      left it half-written.
pub(crate) fn rate_ok(app: &AppState, ip: std::net::IpAddr, limit: u32) -> bool {
    let mut map = app
        .rate
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = std::time::Instant::now();
    // Bound the map: before inserting a brand-new IP into an already-large map, evict
    // every bucket whose window has fully lapsed (those would reset on next use anyway,
    // so dropping them changes no decision — it only reclaims memory).
    if map.len() >= RATE_MAP_SWEEP_AT && !map.contains_key(&ip) {
        map.retain(|_, (_, seen)| now.duration_since(*seen).as_secs() < RATE_WINDOW_SECS);
    }
    let e = map.entry(ip).or_insert((0, now));
    if now.duration_since(e.1).as_secs() >= RATE_WINDOW_SECS {
        *e = (0, now);
    }
    e.0 += 1;
    e.0 <= limit
}

/// The real client IP for rate-limiting, accounting for the Cloudflare Tunnel.
///
/// The deploy (§10.1) fronts the server with `cloudflared`: every request reaches
/// the listener from the tunnel container's single socket address, so `addr.ip()`
/// is ONE IP shared by all clients — keying [`rate_ok`] on it would lump the whole
/// internet into one bucket (one abuser throttles everyone; a global limit is
/// trivially exhausted). Cloudflare sets `CF-Connecting-IP` to the true client
/// address, so we prefer it: a parseable header wins, otherwise we fall back to the
/// socket peer.
///
/// SECURITY — why trusting this header is safe HERE: a client-supplied header is
/// normally spoofable, but the server's port is never directly reachable. The §10.1
/// host binds the app to the box and exposes it ONLY through the Cloudflare Tunnel
/// (the tunnel is the sole ingress; the security group is egress-only — no inbound
/// rule opens the app port), so `cloudflared` is the only thing that can connect,
/// and it overwrites `CF-Connecting-IP` with the verified edge value on every
/// request (a client-sent one can't survive). The dev/in-memory path has no such
/// header and simply falls back to the socket IP — same behaviour as before.
pub(crate) fn client_ip(
    headers: &axum::http::HeaderMap,
    addr: std::net::SocketAddr,
) -> std::net::IpAddr {
    headers
        .get("CF-Connecting-IP")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or_else(|| addr.ip())
}

/// The per-socket pump (1v1 and 2v2 alike — the actor branches on mode).
/// Subscribe to the match actor (which replies with the welcome frame and registers
/// our per-seat sender), send the welcome, then loop: actor-pushed frames go to the
/// socket; socket text goes to the actor and its reply comes back as the ack.
///
/// No shared broadcast: fan-out arrives on `rx`, an
/// **mpsc receiver this socket owns** — the actor addresses it only for this
/// principal, so there is no neighbour to drop a frame for and no shared channel
/// to overflow. A wedged socket grows its own queue (and tears down on a send
/// error) rather than costing anyone else a frame.
///
/// **Absence forfeit (eager detect).** When THIS socket tears down (a ws Close, the
/// stream ending, or a send/recv error), we signal the actor [`seat_vacated`] so it arms
/// the grace timer immediately rather than learning of the drop lazily on the next
/// reconnect. The one exit we DON'T signal is the actor dropping our `rx` sender
/// ([`PumpExit::Superseded`]): that means a reconnect already replaced this socket (or the
/// match ended), so the new socket owns the seat — re-arming `who` here would wrongly
/// forfeit the reconnected player. The signal is best-effort and races nothing: a stray
/// `SeatVacated` arriving after a fresh `Subscribe` is ignored (the principal has a live
/// sender again, and `on_vacated` only arms on a real, still-vacant drop).
async fn socket_loop(
    mut socket: WebSocket,
    entry: Arc<Match>,
    who: Principal,
    name: Option<String>,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<actor::Frame>();
    // Subscribe; the actor plays any pending bot opener and returns the
    // welcome. `None` ⇒ the actor task is gone — close the socket (nothing to vacate).
    let Some(welcome) = entry.handle.subscribe(who, tx, name).await else {
        return;
    };
    let exit = pump(&mut socket, &entry, who, &mut rx, welcome).await;
    // A genuine socket teardown arms the forfeit; a supersede (the actor replaced our
    // sender) does not — the reconnect owns the seat now.
    if matches!(exit, PumpExit::SocketGone) {
        entry.handle.seat_vacated(who).await;
    }
}

/// Why the per-socket pump exited — distinguishes a SOCKET teardown (arm the absence
/// forfeit) from the actor superseding our sender (a reconnect took over; do not arm).
enum PumpExit {
    /// This socket died: a ws Close, the stream ended, or a send/recv error.
    SocketGone,
    /// The actor dropped our `rx` sender — a reconnect superseded us, or the actor
    /// (and match) is shutting down. The new socket owns the seat; don't re-arm.
    Superseded,
}

/// The select loop: push actor frames to the socket; forward socket text to the actor.
/// Returns the [`PumpExit`] reason so the caller can decide whether to arm the forfeit.
async fn pump(
    socket: &mut WebSocket,
    entry: &Arc<Match>,
    who: Principal,
    rx: &mut mpsc::UnboundedReceiver<actor::Frame>,
    welcome: actor::Frame,
) -> PumpExit {
    if socket.send(Message::Text(welcome.into())).await.is_err() {
        return PumpExit::SocketGone;
    }
    loop {
        tokio::select! {
            // A fan-out frame addressed to this principal (Update/TeamUpdate, or the
            // end-of-match SeedRevealed). `None` ⇒ the actor dropped our sender (a
            // supersede or shutdown) — NOT a socket teardown, so don't arm the forfeit.
            pushed = rx.recv() => {
                let Some(json) = pushed else { return PumpExit::Superseded };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    return PumpExit::SocketGone;
                }
            }
            incoming = socket.recv() => {
                // `None`/Close/Err ⇒ this socket is gone — arm the absence forfeit.
                let Some(Ok(Message::Text(text))) = incoming else { return PumpExit::SocketGone };
                // Forward to the actor; its reply is this seat's ack (Applied/
                // TeamApplied/Pong/Welcome/Rejected/Error). `None` ⇒ actor gone.
                let Some(reply) = entry.handle.text(who, text.to_string()).await else {
                    return PumpExit::Superseded;
                };
                if socket.send(Message::Text(reply.into())).await.is_err() {
                    return PumpExit::SocketGone;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use std::net::SocketAddr;

    fn cf_headers(ip: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("CF-Connecting-IP", ip.parse().unwrap());
        h
    }

    /// FIX B: `client_ip` prefers a parseable `CF-Connecting-IP` over the socket
    /// peer, and otherwise falls back to it. The socket addr is the same in both
    /// cases (behind the tunnel it's the one container IP) — only the header decides.
    #[test]
    fn client_ip_prefers_cf_header_then_falls_back_to_the_socket() {
        // One shared socket peer — what every client looks like behind cloudflared.
        let peer: SocketAddr = "10.0.0.1:443".parse().unwrap();

        // The header (the true client IP) wins when present + parseable...
        assert_eq!(
            client_ip(&cf_headers("203.0.113.7"), peer),
            "203.0.113.7".parse::<std::net::IpAddr>().unwrap(),
            "a parseable CF-Connecting-IP is the client IP"
        );
        // ...IPv6 too...
        assert_eq!(
            client_ip(&cf_headers("2001:db8::42"), peer),
            "2001:db8::42".parse::<std::net::IpAddr>().unwrap(),
            "an IPv6 CF-Connecting-IP parses"
        );
        // ...and we fall back to the socket peer with no header (the dev/in-memory path)...
        assert_eq!(
            client_ip(&HeaderMap::new(), peer),
            peer.ip(),
            "no header ⇒ the socket IP (unchanged dev behaviour)"
        );
        // ...or with a garbage header value (never trust it past parsing).
        let mut bad = HeaderMap::new();
        bad.insert("CF-Connecting-IP", "not-an-ip".parse().unwrap());
        assert_eq!(
            client_ip(&bad, peer),
            peer.ip(),
            "an unparseable header falls back to the socket IP"
        );
    }

    /// FIX B (the bug this closes): behind the Cloudflare Tunnel every request shares
    /// ONE socket peer, so keying `rate_ok` on `addr.ip()` would lump all clients into
    /// a single bucket — one throttled client throttles the rest. Keying on
    /// `client_ip(&headers, addr)` gives two distinct `CF-Connecting-IP`s SEPARATE
    /// buckets: exhausting one leaves the other untouched. With no header both collapse
    /// onto the shared socket IP (the old, pre-tunnel behaviour) — proving the header
    /// is exactly what separates them.
    #[test]
    fn cf_header_gives_distinct_clients_separate_rate_buckets() {
        let app = AppState::default();
        // The single tunnel-container peer shared by both clients.
        let peer: SocketAddr = "10.0.0.1:443".parse().unwrap();
        let limit = 3;

        let alice = cf_headers("203.0.113.1");
        let bob = cf_headers("203.0.113.2");

        // Exhaust Alice's bucket (limit allowed, the next refused).
        for _ in 0..limit {
            assert!(
                rate_ok(&app, client_ip(&alice, peer), limit),
                "Alice is under her own limit"
            );
        }
        assert!(
            !rate_ok(&app, client_ip(&alice, peer), limit),
            "Alice is now throttled on her own bucket"
        );

        // Bob — a different CF-Connecting-IP, the SAME socket peer — is unaffected:
        // his first request still passes, so the buckets are genuinely separate.
        assert!(
            rate_ok(&app, client_ip(&bob, peer), limit),
            "a distinct client IP has its own bucket — Alice's throttle didn't touch Bob"
        );

        // Contrast: with NO header, both would key on the shared socket IP and share a
        // bucket. A fresh state, exhausted via the socket fallback, throttles the
        // 'other' client too — the very failure CF-Connecting-IP keying fixes.
        let shared = AppState::default();
        let empty = HeaderMap::new();
        for _ in 0..limit {
            assert!(rate_ok(&shared, client_ip(&empty, peer), limit));
        }
        assert!(
            !rate_ok(&shared, client_ip(&empty, peer), limit),
            "without the header, distinct clients collapse onto one shared socket bucket"
        );
    }

    /// Rate-map memory bound: a flood of distinct source IPs whose windows have all
    /// lapsed must NOT grow the map without limit. A fresh IP resets its bucket in
    /// place but the key would otherwise never be removed, so an attacker rotating
    /// IPs could leak one entry per IP forever. Here the
    /// at-`RATE_MAP_SWEEP_AT` sweep drops every fully-expired bucket on the next
    /// insert, so the live map tracks ACTIVE IPs, not all-time ones. We drive the map
    /// just past the sweep threshold with stale entries, then prove a subsequent
    /// insert collapses it back to (essentially) one live key — the unbounded-growth
    /// DoS closed. (We synthesize stale timestamps directly so the test is instant and
    /// deterministic — no real minute of waiting.)
    #[test]
    fn the_rate_map_is_bounded_and_evicts_expired_buckets() {
        let app = AppState::default();
        // Forge a map of SWEEP_AT fully-expired buckets (last-seen well past the
        // window), exactly as an IP-rotation flood would have accreted under the old
        // never-evict bucket. Done through the same Arc<Mutex> the helper locks.
        let stale =
            std::time::Instant::now() - std::time::Duration::from_secs(RATE_WINDOW_SECS * 4);
        {
            let mut map = app
                .rate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for i in 0..RATE_MAP_SWEEP_AT {
                // Spread synthetic IPs across the v4 space so the keys are distinct.
                let octets = (i as u32).to_be_bytes();
                let ip = std::net::IpAddr::from(octets);
                map.insert(ip, (1, stale));
            }
            assert_eq!(map.len(), RATE_MAP_SWEEP_AT, "the flood accreted");
        }
        // One more request from a brand-new IP trips the sweep: every stale bucket is
        // evicted, leaving just the live newcomer (the map no longer grows unboundedly).
        let fresh: std::net::IpAddr = "203.0.113.250".parse().unwrap();
        assert!(rate_ok(&app, fresh, 30), "the newcomer is under its limit");
        let len_after = app
            .rate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        assert!(
            len_after <= 2,
            "the sweep evicted the expired buckets; the map tracks live IPs only (len={len_after})"
        );
    }

    /// Poison-cascade resilience: a `std::sync::Mutex` poisoned
    /// by a panic-while-held must not turn every later access into a panic — that would
    /// let one unlucky thread cascade into a server-wide DoS. `rate_ok` and
    /// `lock_matches` recover the guard past the poison (the guarded data is plain and
    /// not left half-written), so the server keeps serving. We poison both locks
    /// deliberately and prove each still works afterward.
    #[test]
    fn a_poisoned_lock_does_not_cascade() {
        let app = AppState::default();
        // Poison the rate lock: panic while holding it (caught so the test continues).
        let rate = app.rate.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = rate.lock().unwrap();
            panic!("poison the rate lock");
        }));
        assert!(app.rate.is_poisoned(), "the rate lock is now poisoned");
        // rate_ok recovers the guard instead of panicking — the bucket still works.
        let ip: std::net::IpAddr = "203.0.113.7".parse().unwrap();
        assert!(
            rate_ok(&app, ip, 30),
            "rate_ok serves through a poisoned lock (no cascade)"
        );

        // Same for the match registry: poison it, then prove `lock_matches` recovers.
        let matches = app.matches.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = matches.lock().unwrap();
            panic!("poison the matches lock");
        }));
        assert!(
            app.matches.is_poisoned(),
            "the matches lock is now poisoned"
        );
        assert_eq!(
            crate::lock_matches(&app.matches).len(),
            0,
            "lock_matches reads through a poisoned registry (no cascade)"
        );
    }
}
