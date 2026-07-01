//! The authoritative game server (library + `recollect-server` binary). In-memory
//! sessions degrade gracefully; with `DATABASE_URL` set, Postgres is the source of
//! truth. Endpoints:
//!   GET  /healthz                       liveness/readiness
//!   POST /matches                       create → match id + per-seat tokens
//!   GET  /matches/{id}/ws?token=...      WebSocket for one seat
//!
//! [`run`] is the binary entrypoint; [`serve_in_memory`] is a no-database helper
//! that integration tests (and the bundled CLI's round-trip test) drive. With
//! `STATIC_DIR` set, [`run`] serves the built site + wasm client from this same
//! origin via [`router_with_static`] (the single-origin launch host, tech-design §10.1).
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
mod actor;
mod crypto;
mod identity;
mod match_settings;
mod metrics;
mod session;
mod telemetry;

use actor::{ActorConfig, ActorMode, MatchHandle, Principal};
use crypto::{SeatToken, SeedCommitment};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderName, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use recollect_core::types::{Seat, SeatSlot};
use recollect_journal_postgres::store::{AsyncStore, JOURNAL_SCHEMA, resume_async};
use recollect_protocol::{ClientMsg, PROTOCOL_VERSION, ServerMsg};
use session::Session;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// One hosted match, as the matches map and transport see it. The
/// authoritative state does not live here — a per-match **actor task** owns the
/// [`Session`] by value (no `Mutex<Session>`) and fans per-seat frames out over
/// per-seat mpsc senders (no lossy `broadcast`). This struct keeps only what the
/// HTTP/ws transport needs *before* it talks to the actor: the routing tokens, the
/// lifecycle flag the sweeper reads, and the analytics ids the join-log uses. The
/// actor [`MatchHandle`] is the one channel into the owning task; the session,
/// bot, seed commitment, and finish-record handling all live behind it.
struct Match {
    /// The owning actor's command sender. Cloneable; the last clone dropping ends
    /// the actor task. All command/subscribe traffic goes through here.
    handle: MatchHandle,
    /// 1v1 vs 2v2 — the transport routes a Hello to a [`Principal`] by this.
    kind: MatchKind,
    /// Finished matches mark themselves; the sweeper removes them from the map.
    /// The actor task keeps running until its sockets close (it holds the session),
    /// so an in-flight reveal still lands; eviction only drops the map handle.
    evict: std::sync::atomic::AtomicBool,
    /// UUID for the database record (the u64 id stays the API handle); the journal
    /// stream id. Kept here only for the best-effort join-analytics log.
    db_id: String,
    /// Legacy record (accounts + the `matches` metadata row): used here only for
    /// the spawned join-usage log. The finish-record write moved into the actor.
    journal: Option<std::sync::Arc<tokio::sync::Mutex<recollect_journal_postgres::Journal>>>,
}

/// The per-mode routing data the transport needs to map a Hello token to a seat or
/// slot before handing off to the actor. Tokens are held HASHED — the
/// plaintext was handed to the client once at creation; routing hashes the
/// presented token and compares (constant time).
enum MatchKind {
    OneVsOne {
        token_a: SeatToken,
        token_b: SeatToken,
    },
    TwoVsTwo {
        tokens: [(SeatSlot, SeatToken); 4],
    },
}

/// A server-driven AI opponent occupying one seat (vs-bot 1v1). The actor picks
/// its moves with `recollect_bot::choose_as` off the full engine state and applies
/// them itself — the bot has no socket and no client sequence.
pub(crate) struct Bot {
    pub(crate) seat: Seat,
    pub(crate) difficulty: recollect_bot::Difficulty,
    /// The faction the bot plays — the match's opponent faction (Lorekeeper or Solace).
    pub(crate) faction: recollect_bot::Faction,
    /// The chooser's entropy. A `std` mutex: `choose` is synchronous and the lock
    /// is never held across an `.await` (the actor drops the guard before any append).
    pub(crate) rng: std::sync::Mutex<recollect_core::rng::Rng>,
}

/// A server-driven bot occupying a 2v2 slot — its difficulty, the faction it
/// pilots (its team's), and the opponents' faction, all fed to `choose_as`.
pub(crate) struct SlotBot {
    pub(crate) difficulty: recollect_bot::Difficulty,
    pub(crate) faction: recollect_bot::Faction,
    pub(crate) opp_faction: recollect_bot::Faction,
    pub(crate) rng: std::sync::Mutex<recollect_core::rng::Rng>,
}

/// Where a Hello token routes: a 1v1 seat or a 2v2 slot.
enum Routed {
    Seat(Seat),
    Slot(SeatSlot),
}

impl Routed {
    /// The journal seat label for `match_participants`: 'A'/'B' for a 1v1
    /// seat, 'A1'/'B1'/'A2'/'B2' for a 2v2 slot — the same strings the schema doc
    /// names, so a participant row keys to a recognizable seat.
    fn seat_label(&self) -> &'static str {
        match self {
            Routed::Seat(Seat::A) => "A",
            Routed::Seat(Seat::B) => "B",
            Routed::Slot(SeatSlot::A1) => "A1",
            Routed::Slot(SeatSlot::B1) => "B1",
            Routed::Slot(SeatSlot::A2) => "A2",
            Routed::Slot(SeatSlot::B2) => "B2",
        }
    }
}

#[derive(serde::Deserialize)]
struct NewAccount {
    handle: String,
}

/// POST /accounts — minimal account mechanism: handle -> (account_id, bearer
/// token). The token is shown ONCE; only its hash is stored. Requires the
/// database (503 without it — accounts are inherently durable things).
async fn create_account(
    State(app): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(body): Json<NewAccount>,
) -> impl IntoResponse {
    // Behind the Cloudflare Tunnel the socket peer is one container IP; key the
    // bucket on the true client IP (CF-Connecting-IP) so clients don't share one.
    if !rate_ok(&app, client_ip(&headers, addr), 20) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({"error": "rate limited"})),
        )
            .into_response();
    }
    let Some(j) = app.journal.as_ref() else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "no database configured (set DATABASE_URL)"})),
        )
            .into_response();
    };
    if body.handle.len() < 3
        || body.handle.len() > 24
        || !body
            .handle
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "handle: 3-24 chars, [a-zA-Z0-9_-]"})),
        )
            .into_response();
    }
    match j.lock().await.create_account(&body.handle).await {
        Ok((id, token)) => {
            Json(serde_json::json!({"account_id": id, "token": token})).into_response()
        }
        Err(_) => (
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "handle unavailable"})),
        )
            .into_response(),
    }
}

#[derive(Clone, Default)]
pub struct AppState {
    rate: Arc<Mutex<HashMap<std::net::IpAddr, (u32, std::time::Instant)>>>,
    journal: Option<std::sync::Arc<tokio::sync::Mutex<recollect_journal_postgres::Journal>>>,
    /// The shared authoritative event-journal connection (set with DATABASE_URL).
    event_client: Option<Arc<tokio_postgres::Client>>,
    matches: Arc<Mutex<HashMap<u64, Arc<Match>>>>,
}

/// The live match registry — id → owning actor handle. A `std::sync::Mutex` (the
/// guarded sections are short, synchronous map ops; nothing `.await`s under it).
pub(crate) type MatchMap = Mutex<HashMap<u64, Arc<Match>>>;

/// Lock the match registry, recovering the guard if the lock was poisoned
/// (poison-cascade resilience).
///
/// `std::sync::Mutex` *poisons* on a panic-while-held: every later `lock().unwrap()`
/// then panics too, so a single unlucky thread could cascade into the whole server
/// losing the ability to create, route, recover, or sweep matches — a self-inflicted
/// DoS. The guarded sections here are short, synchronous `HashMap` ops with no
/// `.await` and no partial-write hazard, so a recovered map is sound to keep using:
/// `into_inner()` takes the guard past the poison rather than unwinding. (We assessed
/// `parking_lot` (non-poisoning) too; this keeps the std types and adds no dependency
/// for the one registry that matters.)
pub(crate) fn lock_matches(map: &MatchMap) -> std::sync::MutexGuard<'_, HashMap<u64, Arc<Match>>> {
    map.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

// Seat tokens are minted by `SeatToken::mint` (256 bits of OS entropy, stored
// hashed) — see crypto.rs.

// ── Security response headers (the single-origin §10.1 host) ────────────────
//
// The deploy serves the marketing site + the wasm play client from ONE origin
// ([`router_with_static`]); Cloudflare fronts it (edge TLS + HSTS), but the ORIGIN
// owns the content-security headers so they are version-controlled and tested here,
// not split across an edge config. Applied as an axum middleware so the policy can
// be **path-aware**: the wasm client legitimately needs a looser script policy than
// the static pages.
//
// CSP — two policies, picked by path:
//   • the static site (default): `script-src 'self'` — STRICT, no inline script. The
//     one page that had inline JS (the cards filter) was externalised to `cards.js`
//     for exactly this, so a reflected/stored-script XSS has no inline vector.
//   • the wasm client (`/client/…`): `script-src 'self' 'unsafe-inline'
//     'wasm-unsafe-eval'`. wasm-bindgen's `init()` compiles the module via
//     `WebAssembly` (needs `wasm-unsafe-eval`, NOT the far-broader `unsafe-eval`),
//     and trunk's + the app's bootstraps are inline `<script type=module>` blocks
//     (need `'unsafe-inline'`). `worker-src/child-src blob:` lets wgpu spin its
//     worker; `connect-src 'self'` covers the same-origin `wss://` socket. This is
//     the "trunk-boot" class — the uitest boots the real client under this CSP.
// `style-src` keeps `'unsafe-inline'` on both: the pages carry small `<style>` blocks
// + a couple of layout `style=` attributes (static, no injection vector), and a
// `<style>` element can't be allow-listed without it. Script is where injection bites,
// and script IS locked down.
//
// The other headers are uniform: `nosniff` (no MIME confusion on the served assets),
// `frame-ancestors 'none'` + `X-Frame-Options: DENY` (no clickjacking embed),
// a privacy-preserving `Referrer-Policy`, `COOP: same-origin` (cross-origin isolation
// of the browsing context), a deny-by-default `Permissions-Policy` for powerful APIs
// this app never uses, and a defensive `Strict-Transport-Security` (browsers ignore
// it over plain HTTP — local dev — and honour it once Cloudflare serves HTTPS).
const CSP_SITE: &str = "default-src 'self'; \
script-src 'self'; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data:; \
font-src 'self'; \
connect-src 'self'; \
base-uri 'self'; \
form-action 'self'; \
frame-ancestors 'none'; \
object-src 'none'";

const CSP_CLIENT: &str = "default-src 'self'; \
script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data:; \
font-src 'self'; \
connect-src 'self'; \
worker-src 'self' blob:; \
child-src blob:; \
base-uri 'self'; \
form-action 'self'; \
frame-ancestors 'none'; \
object-src 'none'";

/// Attach the security response headers to every served response. The CSP is chosen
/// by request path: the wasm play client (`/client/…`) gets the wasm-permitting
/// policy, everything else the strict site policy. Header values are static, so the
/// inserts never fail.
async fn security_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::HeaderValue;
    use axum::http::header;
    // Choose the CSP before consuming the request (the path is borrowed from it).
    let is_client = req.uri().path().starts_with("/client/");
    let csp = if is_client { CSP_CLIENT } else { CSP_SITE };

    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(csp),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    h.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), microphone=(), camera=(), payment=(), usb=()"),
    );
    h.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    res
}

#[tracing::instrument(skip_all)]

async fn healthz() -> &'static str {
    "ok"
}

/// The bare API routes (no CORS, no state). The single source of the four
/// endpoints, shared by both surfaces: [`router`] layers permissive CORS over
/// these for the cross-origin dev path; [`router_with_static`] adds the
/// static-file fallback and *no* CORS for the single-origin deploy. Factoring the
/// routes out is what lets the deploy drop the wildcard the dev path needs.
fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/accounts", post(create_account))
        .route("/healthz", get(healthz))
        .route("/matches", post(create_match))
        .route("/matches/{id}/ws", get(ws_handler))
}

/// The **API-only** surface (the dev path), with a permissive CORS layer. Factored
/// so the server binary, the live-ws integration test, and [`serve_in_memory`] wire
/// identical routes onto a shared [`AppState`].
///
/// CORS here is for the **cross-origin dev path** — trunk serves the wasm client on
/// :8088 while this API listens on :8080, two distinct origins, so the browser
/// requires `Access-Control-Allow-Origin` on `POST /matches` / `POST /accounts`
/// (the WebSocket upgrade isn't subject to CORS). `CorsLayer::permissive()` reflects
/// `*`, which is fine for a developer's loopback but too broad to expose publicly —
/// so the DEPLOY path does NOT use it: [`router_with_static`] is single-origin and
/// omits the layer entirely (see its doc). Use this surface only for local dev/tests
/// (or behind a proxy that owns CORS); the production host serves the static site
/// from the same origin via [`router_with_static`].
pub fn router(state: AppState) -> Router {
    api_routes()
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state)
}

/// The API surface plus a static-file fallback that serves the built site + wasm
/// play client from `static_dir` — the single-origin launch host (tech-design §10.1):
/// the page, its assets, and the `wss://…/matches/{id}/ws` socket all share one origin,
/// so there is no CORS and no mixed content, and the deploy needs no separate proxy.
///
/// SECURITY: this path carries **no CORS layer** — deliberately. Because the wasm
/// client targets `window.location.origin`, every request is same-origin and needs
/// no `Access-Control-Allow-Origin` at all; omitting the layer means the deploy never
/// reflects `*` (so no third-party site's JS can drive `POST /matches` / `POST
/// /accounts` cross-origin). The permissive wildcard lives only on the dev-only
/// [`router`].
///
/// The API routes are registered first and take priority; anything they don't match
/// (`/`, `/index.html`, `/client/recollect-web_bg.wasm`, …) falls through to
/// [`tower_http::services::ServeDir`], which infers `Content-Type` (`.wasm` ⇒
/// `application/wasm`) and 404s cleanly for a missing asset. Unknown paths resolve to
/// the directory's `index.html` so a deep link still lands on the site.
pub fn router_with_static(state: AppState, static_dir: std::path::PathBuf) -> Router {
    use tower_http::services::ServeDir;
    // `append_index_html_on_directories` serves `/` → index.html; the not-found
    // fallback keeps an unknown path on the landing page rather than a bare 404.
    let serve = ServeDir::new(&static_dir)
        .append_index_html_on_directories(true)
        .fallback(tower_http::services::ServeFile::new(
            static_dir.join("index.html"),
        ));
    api_routes()
        .fallback_service(serve)
        // The §10.1 host owns its security headers (CSP + nosniff + frame
        // denial + HSTS + a deny-by-default Permissions-Policy). Path-aware so the
        // wasm client gets `wasm-unsafe-eval` while the static pages stay strict.
        .layer(axum::middleware::from_fn(security_headers))
        .with_state(state)
}

/// Serve an in-memory (no-database) instance on an already-bound listener — the
/// graceful degrade path, with no journal/telemetry. For integration tests and
/// local throwaway runs; production goes through [`run`].
pub async fn serve_in_memory(listener: tokio::net::TcpListener) {
    let app = router(AppState::default());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("serve");
}

/// Connect the authoritative event-journal client and migrate the schema. A
/// separate connection from the accounts `Journal`: `AsyncStore` borrows
/// `&Client`, and one pipelined connection serves every match's stream.
async fn connect_event_journal(url: &str) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!(error = %e, "event journal connection closed");
        }
    });
    client.batch_execute(JOURNAL_SCHEMA).await?;
    Ok(client)
}

/// The binary entrypoint: wire telemetry, optional Postgres, the sweeper, and
/// serve until shutdown. `recollect-server`'s `main` is a thin wrapper over this.
pub async fn run() {
    telemetry::init();
    let database_url = std::env::var("DATABASE_URL").ok();
    let journal = match &database_url {
        Some(url) => match recollect_journal_postgres::Journal::connect(url).await {
            Ok(j) => {
                tracing::info!("postgres connected (accounts + match metadata)");
                Some(std::sync::Arc::new(tokio::sync::Mutex::new(j)))
            }
            Err(e) => {
                tracing::error!(error = %e, "DATABASE_URL set but connection failed; running in-memory");
                None
            }
        },
        None => {
            tracing::info!("no DATABASE_URL; in-memory only (accounts disabled)");
            None
        }
    };
    // The authoritative event journal: a second async connection, schema-migrated.
    // Present ⇒ matches are postgres-authoritative (append-before-ack); absent (no
    // DATABASE_URL, or this connect failed) ⇒ they run in-memory (graceful degrade).
    let event_client = match &database_url {
        Some(url) => match connect_event_journal(url).await {
            Ok(c) => {
                tracing::info!("event journal connected (postgres authoritative)");
                Some(Arc::new(c))
            }
            Err(e) => {
                tracing::error!(error = %e, "event journal connect failed; matches run in-memory");
                None
            }
        },
        None => None,
    };
    // Seed the match-id counter past any registered match so a restart never
    // reissues a live handle (recovered matches keep their original id).
    if let Some(j) = journal.as_ref()
        && let Ok(max) = j.lock().await.max_api_id().await
    {
        NEXT_ID.store(max as u64 + 1, Ordering::Relaxed);
    }
    // Sweeper: finished matches leave memory within a minute.
    let sweep_matches: Arc<Mutex<HashMap<u64, Arc<Match>>>> = Default::default();
    {
        let map = sweep_matches.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                lock_matches(&map)
                    .retain(|_, e| !e.evict.load(std::sync::atomic::Ordering::Relaxed));
            }
        });
    }
    let state = AppState {
        rate: Default::default(),
        journal,
        event_client,
        matches: sweep_matches,
    };
    // STATIC_DIR set (the §10.1 deploy host) ⇒ serve the built site + wasm client from
    // this same origin alongside the API; unset (dev, tests, `make server`) ⇒ API only.
    let app = match std::env::var("STATIC_DIR") {
        Ok(dir) if !dir.is_empty() => {
            tracing::info!(static_dir = %dir, "serving the static site from this origin");
            router_with_static(state, std::path::PathBuf::from(dir))
        }
        _ => router(state),
    };
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    tracing::info!(%addr, "recollect-server listening");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;
    use recollect_core::Command;

    /// End-to-end: a journaled 1v1 the server has forgotten (its map entry
    /// gone, as after a restart) is rebuilt from the registry + journal alone by
    /// the real [`recover_match`], and the rebuilt session is bit-identical to the
    /// live one — same seat tokens, same state, same entropy position.
    #[tokio::test]
    #[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-server -- --ignored)"]
    async fn a_forgotten_journaled_match_is_recovered_from_the_journal() {
        let url = std::env::var("PG_URL").expect("PG_URL for the recovery test");

        // The shared authoritative event-journal connection (drives AsyncStore).
        let (raw, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        raw.batch_execute(JOURNAL_SCHEMA).await.unwrap();
        let client = Arc::new(raw);

        // The legacy journal owns the match_registry (its own connection).
        let journal = Arc::new(tokio::sync::Mutex::new(
            recollect_journal_postgres::Journal::connect(&url)
                .await
                .unwrap(),
        ));

        // A live journaled match: open the stream, drive a few journaled turns.
        let seed = 0x0123_4567_89AB_CDEFu64; // positive in i64 ⇒ round-trips exactly
        let api_id = ((std::process::id() as u64) << 8) | 0x09;
        let db_id = format!("recover-test-{api_id}");
        let mut live = Session::new(seed, recollect_bot_deck(), recollect_bot_deck());
        let (genesis, pos) = live.snapshot();
        let store = AsyncStore::open(client.as_ref(), db_id.clone(), &genesis, pos)
            .await
            .unwrap();
        let mut seat = Seat::A;
        let mut seq = [0u64; 2];
        for _ in 0..6 {
            let idx = if seat == Seat::A { 0 } else { 1 };
            seq[idx] += 1;
            if live
                .apply_journaled(&store, Principal::Seat(seat), seq[idx], Command::EndTurn)
                .await
                .is_ok()
            {
                seat = seat.other();
            }
        }

        // Register it (what a restart needs), then build an AppState that does NOT
        // hold the match in memory — exactly the forgotten state recover_match heals.
        // Register the seat-token HASHES (not plaintext) and the commitment
        // salt, exactly as `create_match` does. The original commitment is what a
        // recovered match must still honour, so derive it from the same {seed, salt}.
        let original_commit = crate::crypto::SeedCommitment::new(seed);
        let salt = original_commit.salt();
        journal
            .lock()
            .await
            .register_match(
                api_id as i64,
                &db_id,
                seed as i64,
                &crate::crypto::SeatToken::from_plaintext("tok-a").digest(),
                &crate::crypto::SeatToken::from_plaintext("tok-b").digest(),
                &salt,
                None,
            )
            .await
            .unwrap();
        let app = AppState {
            rate: Default::default(),
            journal: Some(journal.clone()),
            event_client: Some(client.clone()),
            matches: Default::default(),
        };
        assert!(
            app.matches.lock().unwrap().get(&api_id).is_none(),
            "the match starts absent — the server has forgotten it"
        );

        let recovered = recover_match(&app, api_id)
            .await
            .expect("a registered journaled match is recoverable");
        // The registry persists the HASH; the recovered entry rebuilds the
        // handle from it and authorises the original plaintext (rejecting any other).
        // No clear-text seat credential was ever stored or reconstructed.
        let MatchKind::OneVsOne { token_a, token_b } = &recovered.kind else {
            panic!("recovered a 1v1 as something else");
        };
        assert!(token_a.matches("tok-a"));
        assert!(token_b.matches("tok-b"));
        assert!(!token_a.matches("tok-b"), "tokens don't cross-authorise");
        assert!(
            !token_a.matches("a-strangers-token"),
            "a stranger's token fails against the recovered hash"
        );
        assert_eq!(recovered.db_id, db_id);
        // The session is owned by the actor — inspect it through the handle.
        let insp = recovered
            .handle
            .inspect()
            .await
            .expect("the recovered actor answers inspection");
        assert!(insp.bot.is_none(), "a human match recovers without a bot");
        assert_eq!(
            insp.snapshot,
            live.snapshot(),
            "the recovered session reproduces the live state and entropy position"
        );
        // The recovered match re-commits to the ORIGINAL commitment (the
        // registry persisted the salt),
        // so the commitment published before play began is still honourable after a
        // restart — the provably-fair guarantee survives the crash. (Before this, a
        // recovered match re-committed under a FRESH salt and the published
        // commitment could no longer be honoured.)
        assert_eq!(
            insp.seed_commit_hex,
            original_commit.commit_hex(),
            "the recovered commitment matches the one published at creation (same salt)"
        );
        assert!(
            app.matches.lock().unwrap().get(&api_id).is_some(),
            "recovery re-inserts the match so the seat can reconnect"
        );
    }

    /// vs-bot recovery: a registered vs-bot match rebuilds with the Seat-B bot
    /// re-attached at the recorded difficulty, so a solo game survives a restart.
    #[tokio::test]
    #[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-server -- --ignored)"]
    async fn a_forgotten_vs_bot_match_recovers_its_bot() {
        let url = std::env::var("PG_URL").expect("PG_URL for the recovery test");
        let (raw, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        raw.batch_execute(JOURNAL_SCHEMA).await.unwrap();
        let client = Arc::new(raw);
        let journal = Arc::new(tokio::sync::Mutex::new(
            recollect_journal_postgres::Journal::connect(&url)
                .await
                .unwrap(),
        ));

        let seed = 0x00B0_7C0D_E5EE_D101u64;
        let api_id = ((std::process::id() as u64) << 8) | 0xB7;
        let db_id = format!("recover-bot-{api_id}");
        let mut live = Session::new(seed, recollect_bot_deck(), recollect_bot_deck());
        let (genesis, pos) = live.snapshot();
        let store = AsyncStore::open(client.as_ref(), db_id.clone(), &genesis, pos)
            .await
            .unwrap();
        // A couple of journaled turns so there's a stream to replay.
        let _ = live
            .apply_journaled(&store, Principal::Seat(Seat::A), 1, Command::EndTurn)
            .await;

        journal
            .lock()
            .await
            .register_match(
                api_id as i64,
                &db_id,
                seed as i64,
                // Store the seat-token hashes + commitment salt.
                &crate::crypto::SeatToken::from_plaintext("human-a").digest(),
                &crate::crypto::SeatToken::from_plaintext("bot-b").digest(),
                &crate::crypto::SeedCommitment::new(seed).salt(),
                Some("hard"),
            )
            .await
            .unwrap();
        let app = AppState {
            rate: Default::default(),
            journal: Some(journal.clone()),
            event_client: Some(client.clone()),
            matches: Default::default(),
        };

        let recovered = recover_match(&app, api_id)
            .await
            .expect("a registered vs-bot match is recoverable");
        assert!(
            matches!(recovered.kind, MatchKind::OneVsOne { .. }),
            "recovered a vs-bot 1v1 as something else"
        );
        // The Seat-B bot is owned by the actor — inspect it through the handle.
        let insp = recovered
            .handle
            .inspect()
            .await
            .expect("the recovered actor answers inspection");
        let (bot_seat, bot_diff) = insp.bot.expect("the Seat-B bot is re-attached");
        assert_eq!(bot_seat, Seat::B);
        assert_eq!(
            bot_diff,
            recollect_bot::Difficulty::Hard,
            "difficulty restored"
        );
        assert_eq!(
            insp.snapshot,
            live.snapshot(),
            "the recovered state matches the journal"
        );
    }

    /// 2v2 end-to-end: a journaled 2v2 lobby is rebuilt from the registry +
    /// journal by [`recover_match`] — all four slot tokens restored, the team
    /// board and slot rotation bit-identical.
    #[tokio::test]
    #[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-server -- --ignored)"]
    async fn a_forgotten_2v2_lobby_is_recovered_from_the_journal() {
        let url = std::env::var("PG_URL").expect("PG_URL for the recovery test");
        let (raw, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        raw.batch_execute(JOURNAL_SCHEMA).await.unwrap();
        let client = Arc::new(raw);
        let journal = Arc::new(tokio::sync::Mutex::new(
            recollect_journal_postgres::Journal::connect(&url)
                .await
                .unwrap(),
        ));

        let seed = 0x0246_8ACE_1357_9BDFu64;
        let api_id = ((std::process::id() as u64) << 8) | 0x2C;
        let db_id = format!("recover-2v2-{api_id}");
        let mut live = Session::new_2v2(
            seed,
            [
                recollect_bot_deck(),
                recollect_bot_deck(),
                recollect_bot_deck(),
                recollect_bot_deck(),
            ],
        );
        let (genesis, pos) = live.snapshot();
        let store = AsyncStore::open(client.as_ref(), db_id.clone(), &genesis, pos)
            .await
            .unwrap();
        let slot_idx = |s: SeatSlot| match s {
            SeatSlot::A1 => 0usize,
            SeatSlot::B1 => 1,
            SeatSlot::A2 => 2,
            SeatSlot::B2 => 3,
        };
        let mut seq = [0u64; 4];
        for _ in 0..6 {
            let slot = live.snapshot().0.active_slot;
            let i = slot_idx(slot);
            seq[i] += 1;
            let _ = live
                .apply_journaled(&store, Principal::Slot(slot), seq[i], Command::EndTurn)
                .await;
        }

        // Register the four slot-token HASHES + the commitment salt.
        let slot_hashes = [
            crate::crypto::SeatToken::from_plaintext("a1").digest(),
            crate::crypto::SeatToken::from_plaintext("b1").digest(),
            crate::crypto::SeatToken::from_plaintext("a2").digest(),
            crate::crypto::SeatToken::from_plaintext("b2").digest(),
        ];
        let slot_refs: [&[u8]; 4] = [
            &slot_hashes[0],
            &slot_hashes[1],
            &slot_hashes[2],
            &slot_hashes[3],
        ];
        journal
            .lock()
            .await
            .register_match_2v2(
                api_id as i64,
                &db_id,
                seed as i64,
                &slot_refs,
                &crate::crypto::SeedCommitment::new(seed).salt(),
            )
            .await
            .unwrap();
        let app = AppState {
            rate: Default::default(),
            journal: Some(journal.clone()),
            event_client: Some(client.clone()),
            matches: Default::default(),
        };

        let recovered = recover_match(&app, api_id)
            .await
            .expect("a registered 2v2 lobby is recoverable");
        // The registry persisted the four slot-token DIGESTS; recovery
        // rebuilds each `SeatToken` from its hash, and each authorises only its own
        // original plaintext (A1→"a1", …), in slot order — never the clear-text token.
        let MatchKind::TwoVsTwo { tokens } = &recovered.kind else {
            panic!("recovered a 2v2 lobby as something else");
        };
        let plaintexts = ["a1", "b1", "a2", "b2"];
        for ((slot, tok), plain) in tokens.iter().zip(plaintexts) {
            assert!(
                tok.matches(plain),
                "slot {slot:?} token authorises its plaintext {plain}"
            );
        }
        assert_eq!(recovered.db_id, db_id);
        // The session is owned by the actor — inspect it through the handle.
        let insp = recovered
            .handle
            .inspect()
            .await
            .expect("the recovered actor answers inspection");
        assert_eq!(
            insp.snapshot,
            live.snapshot(),
            "the recovered 2v2 lobby reproduces the team board, slot rotation, and entropy"
        );
    }

    // --- the live WebSocket integration test ----------------------------

    /// Create a match over a raw HTTP/1.1 connection (no http-client dep); `query`
    /// is appended to the path (e.g. "?opponent=bot"). `Connection: close` lets us
    /// read the whole response to EOF; the body is the JSON after the headers.
    async fn create_match_http(addr: std::net::SocketAddr, query: &str) -> serde_json::Value {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "POST /matches{query} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8(raw).expect("utf8 http response");
        let body = text
            .split("\r\n\r\n")
            .nth(1)
            .expect("http body after headers");
        serde_json::from_str(body.trim()).expect("json match-creation body")
    }

    async fn send_msg<S>(ws: &mut S, m: &ClientMsg)
    where
        S: futures_util::Sink<tokio_tungstenite::tungstenite::Message> + Unpin,
        S::Error: std::fmt::Debug,
    {
        use futures_util::SinkExt;
        let txt = serde_json::to_string(m).unwrap();
        ws.send(tokio_tungstenite::tungstenite::Message::Text(txt.into()))
            .await
            .expect("ws send");
    }

    /// Next application message (a Text frame), skipping ws-level control frames.
    async fn recv_msg<S>(ws: &mut S) -> ServerMsg
    where
        S: futures_util::Stream<
                Item = Result<
                    tokio_tungstenite::tungstenite::Message,
                    tokio_tungstenite::tungstenite::Error,
                >,
            > + Unpin,
    {
        use futures_util::StreamExt;
        use tokio_tungstenite::tungstenite::Message;
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(txt))) => {
                    return serde_json::from_str(&txt).expect("parse server message");
                }
                Some(Ok(Message::Close(_))) => panic!("server closed the socket mid-game"),
                Some(Ok(_)) => continue, // ws ping/pong/binary/frame
                Some(Err(e)) => panic!("ws stream error: {e:?}"),
                None => panic!("ws stream ended before the game finished"),
            }
        }
    }

    fn view_of(m: ServerMsg) -> recollect_core::view::PlayerView {
        match m {
            ServerMsg::Welcome { view, .. }
            | ServerMsg::Applied { view, .. }
            | ServerMsg::Update { view, .. } => view,
            ServerMsg::Rejected { seq, reason, .. } => {
                panic!("server rejected seq {seq}: {reason}")
            }
            other => panic!("expected a view-bearing message, got {other:?}"),
        }
    }

    /// 2v2 counterpart of [`view_of`]: a slot's `TeamView` + its legal menu.
    fn team_of(
        m: ServerMsg,
    ) -> (
        SeatSlot,
        recollect_core::view::TeamView,
        Vec<recollect_protocol::LegalMove>,
    ) {
        match m {
            ServerMsg::TeamWelcome {
                slot, view, legal, ..
            }
            | ServerMsg::TeamApplied {
                slot, view, legal, ..
            }
            | ServerMsg::TeamUpdate {
                slot, view, legal, ..
            } => (slot, view, legal),
            ServerMsg::Rejected { seq, reason, .. } => {
                panic!("server rejected seq {seq}: {reason}")
            }
            other => panic!("expected a team-view message, got {other:?}"),
        }
    }

    /// A bot client (legal-move-by-phase: pass, answer the hand cap, answer a
    /// choice) plays a full 1v1 to a result over a REAL WebSocket against an
    /// in-memory server — exercising create → connect → Hello/Welcome → the
    /// Cmd/Applied/Update fan-out → finish, the whole transport end to end.
    #[tokio::test]
    async fn a_bot_plays_a_full_1v1_over_the_wire() {
        use recollect_core::state::Phase;
        use recollect_core::types::Seat;

        // In-memory server (no DATABASE_URL): create_match degrades to Engine::apply.
        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let created = create_match_http(addr, "").await;
        let id = created["match_id"].as_u64().expect("match_id");
        let token_a = created["seat_a_token"].as_str().unwrap().to_string();
        let token_b = created["seat_b_token"].as_str().unwrap().to_string();
        // Commit–reveal: the creation response publishes the seed commitment
        // BEFORE any move; we verify the end-of-match reveal against it below.
        let seed_commit = created["seed_commit"]
            .as_str()
            .expect("the response publishes a seed commitment")
            .to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");

        // Handshake each seat: Hello (the routing frame) → the preamble Welcome,
        // then a Ping → Pong round-trip. The seat_loop subscribes to the update
        // fan-out only once it reaches its select loop, and it answers Ping from
        // inside that loop — so a returned Pong proves we're subscribed before any
        // command fires, closing the first-Update race.
        let hello = |tok: &str| ClientMsg::Hello {
            v: PROTOCOL_VERSION,
            match_token: tok.to_string(),
            name: None,
            session_id: None,
        };
        let (mut a, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat A");
        send_msg(&mut a, &hello(&token_a)).await;
        let mut a_view = view_of(recv_msg(&mut a).await);
        send_msg(
            &mut a,
            &ClientMsg::Ping {
                v: PROTOCOL_VERSION,
            },
        )
        .await;
        assert!(matches!(recv_msg(&mut a).await, ServerMsg::Pong { .. }));

        let (mut b, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat B");
        send_msg(&mut b, &hello(&token_b)).await;
        let mut b_view = view_of(recv_msg(&mut b).await);
        send_msg(
            &mut b,
            &ClientMsg::Ping {
                v: PROTOCOL_VERSION,
            },
        )
        .await;
        assert!(matches!(recv_msg(&mut b).await, ServerMsg::Pong { .. }));

        assert_eq!(a_view.seat, Seat::A);
        assert_eq!(b_view.seat, Seat::B);

        let pick = |phase: &Phase| match phase {
            Phase::PendingRelease { .. } => Command::Release { hand_index: 0 },
            Phase::PendingChoice { .. } => Command::Choose { index: 0 },
            _ => Command::EndTurn,
        };

        // Lockstep: the active seat acts (its ack is its own fresh view), the
        // other seat receives the pushed Update. One Cmd ⇒ one Applied + one
        // Update, every time — so we read exactly one from each socket per step.
        let mut seq = [0u64; 2];
        let mut outcome: Option<String> = None;
        for _ in 0..400 {
            if let Phase::Finished {
                result,
                score_a,
                score_b,
            } = &a_view.phase
            {
                outcome = Some(format!("{result:?} {score_a}-{score_b}"));
                break;
            }
            if a_view.active == Seat::A {
                seq[0] += 1;
                let cmd = pick(&a_view.phase);
                send_msg(
                    &mut a,
                    &ClientMsg::Cmd {
                        v: PROTOCOL_VERSION,
                        seq: seq[0],
                        command: cmd,
                    },
                )
                .await;
                a_view = view_of(recv_msg(&mut a).await);
                b_view = view_of(recv_msg(&mut b).await);
            } else {
                seq[1] += 1;
                let cmd = pick(&b_view.phase);
                send_msg(
                    &mut b,
                    &ClientMsg::Cmd {
                        v: PROTOCOL_VERSION,
                        seq: seq[1],
                        command: cmd,
                    },
                )
                .await;
                b_view = view_of(recv_msg(&mut b).await);
                a_view = view_of(recv_msg(&mut a).await);
            }
        }

        let outcome = outcome.expect("the game reached a result within 400 turns over the wire");
        // Both seats observe the same terminal — redaction never alters the score.
        if let (
            Phase::Finished {
                result: ra,
                score_a: aa,
                score_b: ab,
            },
            Phase::Finished {
                result: rb,
                score_a: ba,
                score_b: bb,
            },
        ) = (&a_view.phase, &b_view.phase)
        {
            assert_eq!(format!("{ra:?}"), format!("{rb:?}"), "results agree");
            assert_eq!((aa, ab), (ba, bb), "scores agree");
        } else {
            panic!(
                "both seats must see Finished; A={:?} B={:?}",
                a_view.phase, b_view.phase
            );
        }

        // Commit–reveal: when the match ends the server pushes the seed reveal
        // to BOTH seats. Read it off each socket and verify it reproduces the
        // commitment published in the creation response — a provably-fair shuffle.
        for sock in [&mut a, &mut b] {
            let reveal = loop {
                match recv_msg(sock).await {
                    ServerMsg::SeedRevealed {
                        seed,
                        salt_hex,
                        commit_hex,
                        ..
                    } => break (seed, salt_hex, commit_hex),
                    // Tolerate a trailing Update/Applied still in flight before the reveal.
                    _ => continue,
                }
            };
            assert_eq!(
                reveal.2, seed_commit,
                "the revealed commitment equals the one published at creation"
            );
            assert!(
                crate::crypto::SeedCommitment::verify(&seed_commit, reveal.0, &reveal.1),
                "sha256(seed‖salt) matches the published commit — the shuffle was fixed before play"
            );
        }
        println!("over-the-wire 1v1 finished: {outcome}");
    }

    /// The `Hello.name` is captured for the table (the public `seat_names`
    /// roster) WITHOUT leaking into the opponent's redacted view. Seat A connects
    /// under a distinctive handle; Seat B's `Welcome` carries A's name only in the
    /// public `seat_names` field — the redacted `PlayerView` body B sees has no
    /// trace of it (the name is public, the hands/seed are not). This guards that
    /// name-tagging a match doesn't widen what a client sees of its opponent.
    ///
    /// (The journal write — `match_participants` — is best-effort and DB-gated; the
    /// in-memory server has no journal, so the durable round-trip is proven by the
    /// journal crate's `match_participants_record_the_name_anon_and_signed_in`
    /// (`make db-test`). This test owns the wire/redaction half.)
    #[tokio::test]
    async fn the_session_name_reaches_the_roster_but_not_the_opponent_view() {
        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let created = create_match_http(addr, "").await;
        let id = created["match_id"].as_u64().expect("match_id");
        let token_a = created["seat_a_token"].as_str().unwrap().to_string();
        let token_b = created["seat_b_token"].as_str().unwrap().to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");

        // A handle unlikely to occur incidentally in a serialized view.
        const A_NAME: &str = "Zephyrine-Quux-7";
        let hello_named = |tok: &str, n: Option<&str>| ClientMsg::Hello {
            v: PROTOCOL_VERSION,
            match_token: tok.to_string(),
            name: n.map(|s| s.to_string()),
            session_id: Some("sid-redaction".into()),
        };

        // Seat A connects under its name.
        let (mut a, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat A");
        send_msg(&mut a, &hello_named(&token_a, Some(A_NAME))).await;
        let _ = recv_msg(&mut a).await; // A's Welcome

        // Seat B connects (anonymous). Its Welcome's roster names A; its view does not.
        let (mut b, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat B");
        send_msg(&mut b, &hello_named(&token_b, None)).await;
        match recv_msg(&mut b).await {
            ServerMsg::Welcome {
                seat,
                view,
                seat_names,
                ..
            } => {
                assert_eq!(seat, Seat::B);
                // The public roster carries A's chosen handle (the table sees names).
                assert!(
                    seat_names.iter().flatten().any(|n| n == A_NAME),
                    "the opponent's name is on the public roster: {seat_names:?}"
                );
                // The REDACTED view body has no trace of A's name — name-tagging the
                // match never leaked it into what B sees of the opponent.
                let view_json = serde_json::to_string(&view).unwrap();
                assert!(
                    !view_json.contains(A_NAME),
                    "the opponent's name must not appear in the redacted PlayerView"
                );
            }
            other => panic!("expected B's Welcome, got {other:?}"),
        }
    }

    /// Server AI seat: a single human client plays a full 1v1 against the
    /// SERVER's bot (`?opponent=bot`). The server drives Seat B after every human
    /// move, so each ack already reflects the bot's reply and the match reaches a
    /// result with only one socket connected — which it could not do unless the
    /// server were driving B (otherwise it would stall on B's turn).
    #[tokio::test]
    async fn a_human_plays_a_full_1v1_against_the_server_bot() {
        use recollect_core::state::Phase;
        use recollect_core::types::Seat;

        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let created = create_match_http(addr, "?opponent=bot&difficulty=hard").await;
        assert_eq!(created["opponent"], "bot");
        assert!(
            created["seat_b_token"].is_null(),
            "the bot holds seat B; its token is never handed out"
        );
        let id = created["match_id"].as_u64().expect("match_id");
        let token_a = created["seat_a_token"].as_str().unwrap().to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");

        let (mut a, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat A");
        send_msg(
            &mut a,
            &ClientMsg::Hello {
                v: PROTOCOL_VERSION,
                match_token: token_a,
                name: None,
                session_id: None,
            },
        )
        .await;
        let mut view = view_of(recv_msg(&mut a).await);
        assert_eq!(view.seat, Seat::A);

        let pick = |phase: &Phase| match phase {
            Phase::PendingRelease { .. } => Command::Release { hand_index: 0 },
            Phase::PendingChoice { .. } => Command::Choose { index: 0 },
            _ => Command::EndTurn,
        };

        let mut outcome: Option<String> = None;
        for turn in 1u64..=400 {
            if let Phase::Finished {
                result,
                score_a,
                score_b,
            } = &view.phase
            {
                outcome = Some(format!("{result:?} {score_a}-{score_b}"));
                break;
            }
            // The bot resolves Seat B's whole turn before we're acked, so it is
            // always our move when a non-terminal view comes back.
            assert_eq!(
                view.active,
                Seat::A,
                "the server bot took Seat B's turn; it is ours again"
            );
            // `turn` is a strictly increasing client sequence (gaps are fine).
            send_msg(
                &mut a,
                &ClientMsg::Cmd {
                    v: PROTOCOL_VERSION,
                    seq: turn,
                    command: pick(&view.phase),
                },
            )
            .await;
            view = view_of(recv_msg(&mut a).await);
        }
        let outcome =
            outcome.expect("the match vs the server bot reached a result within 400 turns");
        println!("vs-server-bot finished: {outcome}");
    }

    /// 2v2 over the wire: a slot connects, gets its `TeamView` on the 6×6 board
    /// with the active slot's legal menu, plays a command, and the turn rotates —
    /// the path the CLI and web 2v2 clients drive.
    #[tokio::test]
    async fn a_2v2_slot_plays_over_the_wire() {
        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        // Pin a deterministic A1-opener so the 4-way 2v2 toss can't flake this turn-rotation
        // test (all-human → equal weights). The first seed that opens A1, passed via `?seed=`.
        let pin = (0u64..)
            .find(|&s| {
                recollect_core::quickplay::decide_opener_2v2(s, [100, 100, 100, 100])
                    == SeatSlot::A1
            })
            .unwrap();
        let created = create_match_http(addr, &format!("?mode=2v2&seed={pin}")).await;
        assert_eq!(created["mode"], "2v2");
        let id = created["match_id"].as_u64().expect("match_id");
        let a1 = created["slot_tokens"]["A1"].as_str().unwrap().to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");

        let (mut s, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect slot A1");
        send_msg(
            &mut s,
            &ClientMsg::Hello {
                v: PROTOCOL_VERSION,
                match_token: a1,
                name: None,
                session_id: None,
            },
        )
        .await;

        // The preamble TeamWelcome: A1's 6×6 board with A1 active and a real menu.
        let (slot, view, legal) = team_of(recv_msg(&mut s).await);
        assert_eq!(slot, SeatSlot::A1);
        assert_eq!(view.active_slot, SeatSlot::A1);
        assert_eq!(view.board_w, 6, "the 2v2 board is 6×6");
        assert!(
            !legal.is_empty(),
            "the opening slot has its legal moves over the wire"
        );

        // Pass; the ack shows the turn rotated to B1 and A1 now has no moves.
        send_msg(
            &mut s,
            &ClientMsg::Cmd {
                v: PROTOCOL_VERSION,
                seq: 1,
                command: Command::EndTurn,
            },
        )
        .await;
        let (_, view2, legal2) = team_of(recv_msg(&mut s).await);
        assert_eq!(
            view2.active_slot,
            SeatSlot::B1,
            "the slot rotation advanced"
        );
        assert!(legal2.is_empty(), "not A1's turn — no legal moves for it");
    }

    /// The 2v2-vs-bot opening drive, end to end over a REAL WebSocket: a lobby
    /// with one human (A1) versus three bots (A2, B1, B2) whose seeded opener is a
    /// BOT slot. The human connects and its very first `TeamWelcome` already shows
    /// ITS OWN turn with a real legal menu — the server drove the bot opener (and
    /// the bot allies) before the welcome, so the human is never stranded watching
    /// the opponent's turn with nothing to do. This is the 1v1 `vs_bot` parity the
    /// 2v2 welcome lacked. The whole transport path: create → Hello → TeamWelcome.
    #[tokio::test]
    async fn a_2v2_vs_bot_opener_auto_plays_before_the_human_connects() {
        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        // Pin a seed whose opener is a BOT slot, so the drive is genuinely exercised
        // (not an A1-opens-anyway accident). The bot slots carry initiative weight on
        // top of the base 100, which only favours a bot opener further — so a seed
        // whose EQUAL-weight toss already opens a non-A1 slot opens a bot slot under
        // the real (bot-heavier) weights too. A1 is the lone human, so the drive must
        // roll every bot slot and land the turn back on A1.
        let pin = (0u64..)
            .find(|&s| {
                recollect_core::quickplay::decide_opener_2v2(s, [100, 100, 100, 100])
                    != SeatSlot::A1
            })
            .unwrap();
        let created = create_match_http(
            addr,
            &format!("?mode=2v2&seats=human,bot,bot,bot&seed={pin}"),
        )
        .await;
        assert_eq!(created["mode"], "2v2");
        let id = created["match_id"].as_u64().expect("match_id");
        let a1 = created["slot_tokens"]["A1"].as_str().unwrap().to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");

        let (mut s, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect slot A1");
        send_msg(
            &mut s,
            &ClientMsg::Hello {
                v: PROTOCOL_VERSION,
                match_token: a1,
                name: None,
                session_id: None,
            },
        )
        .await;

        // The human's FIRST frame: it is A1's turn already, with a real menu.
        let (slot, view, legal) = team_of(recv_msg(&mut s).await);
        assert_eq!(slot, SeatSlot::A1, "the human is welcomed on its own slot");
        assert_eq!(
            view.active_slot,
            SeatSlot::A1,
            "the bot opener and bot allies auto-played server-side; the turn is the \
             human's — it is NOT stranded on the opponent's turn"
        );
        assert!(
            !legal.is_empty(),
            "the human's first view carries its legal menu — it can act immediately"
        );
    }

    /// First-class reconnection, end to end over a REAL WebSocket: a seat
    /// plays a move, its socket DROPS (closed), it reconnects on a fresh socket
    /// with the SAME seat token, and the resume `Welcome` carries the current
    /// authoritative state (reflecting the pre-drop move). The token gate is
    /// proven both ways: the correct token resumes; a stranger's token is rejected
    /// with `bad_token` and never sees the seat. This is the full transport path
    /// the web + CLI clients drive — Hello-routing, the actor's supersede on
    /// re-subscribe, and the hashed-token authorisation, all over the wire.
    #[tokio::test]
    async fn a_seat_drops_and_reconnects_with_its_token_over_the_wire() {
        use recollect_core::types::Seat;

        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        // A PvP match (no bot) so Seat A's move leaves the turn with Seat B — a
        // visible state change the resume must reflect. Pin a seed that opens Seat
        // A (the toss is otherwise CSPRNG-seeded and would flake) via `?seed=`.
        let pin = (0u64..)
            .find(|&s| recollect_core::quickplay::decide_opener(s, 0) == Seat::A)
            .unwrap();
        let created = create_match_http(addr, &format!("?seed={pin}")).await;
        let id = created["match_id"].as_u64().expect("match_id");
        let token_a = created["seat_a_token"].as_str().unwrap().to_string();
        let token_b = created["seat_b_token"].as_str().unwrap().to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");
        let hello = |tok: &str| ClientMsg::Hello {
            v: PROTOCOL_VERSION,
            match_token: tok.to_string(),
            name: None,
            session_id: None,
        };

        // Seat A connects, gets its Welcome (Seat A opens on this seed-source), and
        // ends its turn — the authoritative state now has Seat B active.
        let (mut a, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat A");
        send_msg(&mut a, &hello(&token_a)).await;
        let v0 = view_of(recv_msg(&mut a).await);
        assert_eq!(v0.seat, Seat::A);
        assert_eq!(v0.active, Seat::A, "Seat A opens");
        send_msg(
            &mut a,
            &ClientMsg::Cmd {
                v: PROTOCOL_VERSION,
                seq: 1,
                command: Command::EndTurn,
            },
        )
        .await;
        let v1 = view_of(recv_msg(&mut a).await);
        assert_eq!(v1.active, Seat::B, "EndTurn passed the turn to B");

        // Seat A's socket DROPS — a mid-match disconnect (close the ws).
        a.close(None).await.ok();
        drop(a);

        // A stranger CANNOT hijack the seat: a wrong token is rejected with
        // bad_token and never receives a Welcome/view. (Token-gating preserved.)
        let (mut imposter, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect imposter");
        send_msg(&mut imposter, &hello("deadbeef-not-a-real-token")).await;
        match recv_msg(&mut imposter).await {
            ServerMsg::Error { message, .. } => assert_eq!(
                message, "bad_token",
                "a stranger's token is refused — the hashed seat token gates the rejoin"
            ),
            other => panic!("expected a bad_token Error, got {other:?}"),
        }
        drop(imposter);

        // Seat A reconnects on a FRESH socket with the SAME token. The resume
        // Welcome carries the CURRENT state — Seat B active, reflecting the
        // pre-drop EndTurn — not a stale opening snapshot.
        let (mut a2, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("reconnect seat A");
        send_msg(&mut a2, &hello(&token_a)).await;
        match recv_msg(&mut a2).await {
            ServerMsg::Welcome { seat, view, .. } => {
                assert_eq!(seat, Seat::A, "the resume re-seats A on its own seat");
                assert_eq!(
                    view.active,
                    Seat::B,
                    "the resume view reflects the move made before the drop"
                );
                assert_eq!(view.seat, Seat::A, "A's own redacted view");
            }
            other => panic!("expected a resume Welcome, got {other:?}"),
        }

        // The resumed socket is fully live: Seat B (connecting fresh) ends its
        // turn, and the actor fans the reconnected Seat A its Update — proving
        // fan-out re-addressed to the new socket.
        let (mut b, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat B");
        send_msg(&mut b, &hello(&token_b)).await;
        let _ = recv_msg(&mut b).await; // B's Welcome
        send_msg(
            &mut b,
            &ClientMsg::Cmd {
                v: PROTOCOL_VERSION,
                seq: 1,
                command: Command::EndTurn,
            },
        )
        .await;
        let _ = recv_msg(&mut b).await; // B's own Applied
        // A's resumed socket receives the pushed Update (turn back to A).
        let pushed = view_of(recv_msg(&mut a2).await);
        assert_eq!(pushed.seat, Seat::A, "A's own view on the resumed socket");
        assert_eq!(pushed.active, Seat::A, "the turn returned to A");
    }

    /// A raw GET that returns the full (status-line + headers + body) response text,
    /// so a static-serving assertion can read both the `Content-Type` and the body.
    async fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        String::from_utf8(raw).expect("utf8 http response")
    }

    /// A raw GET carrying an `Origin` (a cross-origin request, what a browser sends),
    /// returning the response's header block lowercased — so a CORS assertion can
    /// check whether `access-control-allow-origin` was emitted.
    async fn http_get_with_origin(addr: std::net::SocketAddr, path: &str, origin: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: {origin}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8(raw).expect("utf8 http response");
        // The header block is everything before the blank line; lowercase it so the
        // header-name check is case-insensitive (HTTP header names are).
        text.split("\r\n\r\n")
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    /// Spawn a router on an ephemeral loopback port; returns its address + the serve
    /// task handle (abort it to stop). Shared by the CORS-surface assertions.
    async fn spawn_router(app: Router) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });
        (addr, handle)
    }

    /// SECURITY (FIX A — production CORS): the two surfaces differ exactly where the
    /// deploy needs them to. The dev-only [`router`] reflects `Access-Control-Allow-Origin`
    /// for a cross-origin request (trunk on :8088 → API on :8080 must `fetch` POST
    /// /matches); the single-origin DEPLOY [`router_with_static`] emits NO such header
    /// (the wasm client is same-origin, so no third-party site's JS can drive the API
    /// cross-origin). Both share the same `api_routes`, so this pins the *only* intended
    /// difference: the permissive wildcard is present on dev, absent on the deploy path.
    #[tokio::test]
    async fn cors_is_dev_only_and_absent_on_the_single_origin_deploy_path() {
        const ORIGIN: &str = "https://evil.example";

        // Dev surface: permissive CORS reflects the wildcard on a simple cross-origin GET.
        let (dev_addr, dev) = spawn_router(router(AppState::default())).await;
        let dev_headers = http_get_with_origin(dev_addr, "/healthz", ORIGIN).await;
        assert!(
            dev_headers.contains("access-control-allow-origin"),
            "the dev router (cross-origin trunk → API) must emit Access-Control-Allow-Origin: \
             {dev_headers}"
        );
        dev.abort();

        // Deploy surface: a static dir wired, NO CORS layer ⇒ no such header at all.
        let dir = std::env::temp_dir().join(format!("recollect-cors-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            b"<!doctype html><title>Recollect</title>",
        )
        .unwrap();
        let (dep_addr, dep) =
            spawn_router(router_with_static(AppState::default(), dir.clone())).await;
        let dep_headers = http_get_with_origin(dep_addr, "/healthz", ORIGIN).await;
        assert!(
            !dep_headers.contains("access-control-allow-origin"),
            "the single-origin deploy router must NOT emit any Access-Control-Allow-Origin \
             (it serves the wasm client same-origin): {dep_headers}"
        );
        dep.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// §10.1 single-origin host: with a static dir wired, the SAME server serves the
    /// built site (`/` → index.html) and the wasm client (`.wasm` ⇒ `application/wasm`),
    /// while the API routes still answer — so the site and the `wss` socket share one
    /// origin (no CORS, no separate proxy). Guards the deploy's static-serving path.
    #[tokio::test]
    async fn static_dir_serves_the_site_alongside_the_api() {
        let dir = std::env::temp_dir().join(format!("recollect-static-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("client")).unwrap();
        std::fs::write(
            dir.join("index.html"),
            b"<!doctype html><title>Recollect</title>",
        )
        .unwrap();
        std::fs::write(dir.join("client/recollect-web_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();

        let app = router_with_static(AppState::default(), dir.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        // The API still answers on the static-serving router.
        let health = http_get(addr, "/healthz").await;
        assert!(
            health.contains("200 OK") && health.contains("ok"),
            "healthz: {health}"
        );
        // `/` falls through to index.html.
        let root = http_get(addr, "/").await;
        assert!(
            root.contains("200 OK") && root.contains("Recollect"),
            "root: {root}"
        );
        // The wasm client is served with the correct MIME (the import would 404 otherwise).
        let wasm = http_get(addr, "/client/recollect-web_bg.wasm").await;
        assert!(wasm.contains("200 OK"), "wasm status: {wasm}");
        assert!(
            wasm.to_ascii_lowercase()
                .contains("content-type: application/wasm"),
            "wasm must be served as application/wasm: {wasm}"
        );

        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SECURITY: the single-origin deploy host emits the security response
    /// headers on every served response, with a **path-aware CSP** — the strict
    /// `script-src 'self'` for the static site, the wasm-permitting
    /// `'wasm-unsafe-eval'` (NEVER the broad `unsafe-eval`) for the `/client/` wasm
    /// app. Guards the headers against silent regression (a launch gate). The dev-only
    /// in-memory [`router`] is deliberately unadorned (loopback) — only the deploy path
    /// ([`router_with_static`]) carries these.
    #[tokio::test]
    async fn the_deploy_host_sets_security_headers_with_a_path_aware_csp() {
        let dir = std::env::temp_dir().join(format!("recollect-sec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("client")).unwrap();
        std::fs::write(
            dir.join("index.html"),
            b"<!doctype html><title>Recollect</title>",
        )
        .unwrap();
        std::fs::write(
            dir.join("client/index.html"),
            b"<!doctype html><title>play</title>",
        )
        .unwrap();

        let (addr, handle) =
            spawn_router(router_with_static(AppState::default(), dir.clone())).await;

        // The static landing page: the full header set, the STRICT site CSP.
        let site = http_get(addr, "/").await.to_ascii_lowercase();
        for needle in [
            "x-content-type-options: nosniff",
            "x-frame-options: deny",
            "referrer-policy: strict-origin-when-cross-origin",
            "cross-origin-opener-policy: same-origin",
            "permissions-policy:",
            "strict-transport-security: max-age=31536000",
            "content-security-policy:",
            "frame-ancestors 'none'",
            "object-src 'none'",
        ] {
            assert!(
                site.contains(needle),
                "site response missing `{needle}`:\n{site}"
            );
        }
        // Site script policy is locked to self — no inline-script / eval vector.
        assert!(
            site.contains("script-src 'self';") || site.contains("script-src 'self' "),
            "the site CSP must be a strict script-src 'self' (no unsafe-*):\n{site}"
        );
        assert!(
            !site.contains("wasm-unsafe-eval") && !site.contains("unsafe-eval"),
            "the static site must NOT permit any eval:\n{site}"
        );

        // The wasm client page: the wasm-permitting CSP (so the trunk-boot works),
        // but still NOT the dangerous broad `unsafe-eval`.
        let client = http_get(addr, "/client/").await.to_ascii_lowercase();
        assert!(
            client.contains("content-security-policy:"),
            "the client must also carry a CSP:\n{client}"
        );
        assert!(
            client.contains("'wasm-unsafe-eval'"),
            "the wasm client CSP must allow wasm-unsafe-eval (wasm-bindgen init compiles \
             the module):\n{client}"
        );
        assert!(
            !client.contains("'unsafe-eval'") || client.contains("'wasm-unsafe-eval'"),
            "the client must use the narrow wasm-unsafe-eval, never the broad unsafe-eval"
        );
        // The narrow grant is wasm-only: the literal ` unsafe-eval` (space-prefixed, so
        // not the `wasm-` form) must be absent.
        assert!(
            !client.contains(" 'unsafe-eval'"),
            "the client must not carry the broad unsafe-eval:\n{client}"
        );

        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Absence forfeit, end to end over a REAL WebSocket: a PvP match with a tiny
    /// `?abandon_grace_secs=1`. Seat A connects, Seat B connects, then Seat A's socket
    /// CLOSES and never returns. The transport's `socket_loop` signals the actor on
    /// teardown, the grace lapses, and the actor issues `Command::MatchAbandoned` against
    /// Seat A — so the present Seat B is pushed the finished view (`Win(B)`) plus the
    /// end-of-match seed reveal, all without B sending a single command. This is the whole
    /// disconnect→grace→forfeit wiring over the wire (the `seat_vacated` eager-detect path
    /// the actor-level tests can't reach). A 1s grace keeps it fast yet realistic.
    #[tokio::test]
    async fn a_disconnect_forfeits_the_match_over_the_wire() {
        use recollect_core::state::Phase;
        use recollect_core::types::Seat;

        let app = router(AppState::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        });

        // Pin a seed that opens Seat A (otherwise the toss is CSPRNG-seeded and flakes),
        // and a 1-second absence-forfeit grace.
        let pin = (0u64..)
            .find(|&s| recollect_core::quickplay::decide_opener(s, 0) == Seat::A)
            .unwrap();
        let created = create_match_http(addr, &format!("?seed={pin}&abandon_grace_secs=1")).await;
        let id = created["match_id"].as_u64().expect("match_id");
        let token_a = created["seat_a_token"].as_str().unwrap().to_string();
        let token_b = created["seat_b_token"].as_str().unwrap().to_string();
        let seed_commit = created["seed_commit"].as_str().unwrap().to_string();
        let ws_url = format!("ws://{addr}/matches/{id}/ws");
        let hello = |tok: &str| ClientMsg::Hello {
            v: PROTOCOL_VERSION,
            match_token: tok.to_string(),
            name: None,
            session_id: None,
        };

        // Both seats connect (so the actor holds B's sender for the forfeit fan-out).
        let (mut a, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat A");
        send_msg(&mut a, &hello(&token_a)).await;
        let _ = view_of(recv_msg(&mut a).await); // A's Welcome
        let (mut b, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .expect("connect seat B");
        send_msg(&mut b, &hello(&token_b)).await;
        let _ = view_of(recv_msg(&mut b).await); // B's Welcome

        // Seat A's socket CLOSES and never returns — the transport arms the forfeit.
        a.close(None).await.ok();
        drop(a);

        // Within the grace + slack, Seat B is pushed the forfeit finish (Win(B)) and the
        // seed reveal — read frames until both arrive (tolerating any in-flight frames).
        let (mut saw_finish, mut saw_reveal) = (false, false);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while (!saw_finish || !saw_reveal) && tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_secs(5), recv_msg(&mut b)).await {
                Ok(ServerMsg::Update { view, .. }) => {
                    if let Phase::Finished { result, .. } = &view.phase {
                        assert_eq!(
                            format!("{result:?}"),
                            "Win(B)",
                            "the disconnected Seat A forfeits; the present Seat B wins"
                        );
                        saw_finish = true;
                    }
                }
                Ok(ServerMsg::SeedRevealed { commit_hex, .. }) => {
                    assert_eq!(
                        commit_hex, seed_commit,
                        "the forfeit reveal matches the commitment published at creation"
                    );
                    saw_reveal = true;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(
            saw_finish,
            "the present opponent received the forfeit finish over the wire"
        );
        assert!(
            saw_reveal,
            "the end-of-match seed reveal reached the present opponent"
        );
    }
}

mod matchmaking;
pub(crate) use matchmaking::*;

mod ws;
pub(crate) use ws::*;
