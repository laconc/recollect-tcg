//! Postgres adapter. Two journals live here:
//!
//! - [`store`] — the **authoritative** event journal (the family swap, now
//!   landed): `execute_async` appends-before-ack via ironstate's
//!   `prepare`/`commit`/`abort` over a generic `journal_events`/`journal_snapshots`
//!   schema, `resume_async` rebuilds from the head, and the seven-property
//!   `journal_contract_test!` bar is met by a synchronous `Journal` twin
//!   (`tests/journal_contract.rs`). The server drives this through
//!   `Session::apply_journaled` when `DATABASE_URL` is set.
//! - This module's [`Journal`] — what's *left* for the legacy path: accounts and
//!   the `matches` metadata row (seed, result). The old per-command `match_events`
//!   record is retired now that `journal_events` is authoritative; the table and
//!   its methods remain only until the accounts/match rows move onto the family
//!   journal too.
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
pub mod store;

use sha2::{Digest, Sha256};
use tokio_postgres::{Client, NoTls};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    handle      TEXT NOT NULL UNIQUE,
    token_hash  BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS matches (
    id          UUID PRIMARY KEY,
    seed        BIGINT NOT NULL,
    account_a   UUID REFERENCES accounts(id),
    account_b   UUID REFERENCES accounts(id),
    result      TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS match_events (
    match_id        UUID NOT NULL REFERENCES matches(id),
    seq             BIGINT NOT NULL,
    payload         BYTEA NOT NULL,          -- postcard, one command's batch
    draws_after     BIGINT NOT NULL,         -- entropy position after this batch
    schema_version  INT NOT NULL,
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (match_id, seq)
);
-- Cross-restart recovery: enough to rebuild a live match's session and let
-- a seat reconnect after a server restart. `api_id` is the u64 API handle; the
-- game state itself is replayed from the `journal_events` stream keyed by `db_id`.
-- Seat tokens are short-lived but must outlive a restart, so they live here —
-- stored HASHED: the columns hold `sha256(token)`, never the plaintext,
-- so a database read can't leak a live seat credential (mirrors `accounts`'
-- `token_hash` + the running server's in-memory `SeatToken`). `mode` is '1v1'
-- (token_a/token_b = seats A/B) or '2v2' (token_a/token_b = slots A1/B1,
-- token_a2/token_b2 = slots A2/B2). `seed_salt` is the commit–reveal salt:
-- persisting it lets a recovered match re-commit under the ORIGINAL
-- salt, so its published commitment stays honourable across a restart. The salt
-- is secret until the end-of-match reveal — it lives ONLY here (server-side),
-- never in a view or event.
CREATE TABLE IF NOT EXISTS match_registry (
    api_id        BIGINT PRIMARY KEY,
    db_id         TEXT   NOT NULL,
    seed          BIGINT NOT NULL,
    token_a_hash  BYTEA  NOT NULL,
    token_b_hash  BYTEA  NOT NULL,
    seed_salt     BYTEA  NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Evolve the table in place (no migration framework): 2v2 adds a mode tag and
-- the two extra slot-token hashes. `ADD COLUMN IF NOT EXISTS` is idempotent on
-- connect. The hash/salt columns are added nullable here (an existing table may
-- hold stale rows from before the migration); fresh databases get them NOT NULL
-- from the CREATE above, and the writer always supplies them — so a live match's
-- row is never missing one.
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS mode          TEXT NOT NULL DEFAULT '1v1';
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS token_a_hash  BYTEA;
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS token_b_hash  BYTEA;
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS token_a2_hash BYTEA;
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS token_b2_hash BYTEA;
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS seed_salt     BYTEA;
-- The hash columns above are authoritative; drop any plaintext seat-token columns
-- (a live credential must never sit in the clear). `DROP COLUMN IF EXISTS` is
-- idempotent: a no-op once dropped, and a no-op on a fresh DB that never had them.
ALTER TABLE match_registry DROP COLUMN IF EXISTS token_a;
ALTER TABLE match_registry DROP COLUMN IF EXISTS token_b;
ALTER TABLE match_registry DROP COLUMN IF EXISTS token_a2;
ALTER TABLE match_registry DROP COLUMN IF EXISTS token_b2;
-- vs-bot 1v1: the difficulty of the server-driven Seat B, so recovery can
-- re-attach the bot. NULL ⇒ a human-vs-human match.
ALTER TABLE match_registry ADD COLUMN IF NOT EXISTS bot_difficulty TEXT;
-- Usage analytics: a lightweight product-event log keyed by an anonymous session id
-- (no PII, no account). Lets a player's journey be reconstructed across matches.
CREATE TABLE IF NOT EXISTS usage_events (
    id          BIGSERIAL PRIMARY KEY,
    ts          TIMESTAMPTZ NOT NULL DEFAULT now(),
    session_id  TEXT,
    event       TEXT NOT NULL,
    match_id    TEXT
);
CREATE INDEX IF NOT EXISTS usage_events_session_idx ON usage_events (session_id, ts);
-- Who played each match, name-tagged. One row per occupied seat — anonymous
-- and signed-in alike — so the event stream is complete, handle-in-use training
-- data from day one. `handle` is the name a seat connected under (a server fallback
-- when the client sent none); `session_id` is the anonymous identity (the same
-- opaque id `usage_events` keys on); `account_id` is NULL at launch and is the
-- forward-compatible seam filled when an account *claims* a handle — "who
-- played" is a handle an account can later own. `seat` is 'A'/'B' (1v1) or
-- 'A1'/'B1'/'A2'/'B2' (2v2). The seat is the conflict key so a reconnect (a fresh
-- `Hello` on the same seat) refreshes the handle rather than duplicating the row.
CREATE TABLE IF NOT EXISTS match_participants (
    match_id    TEXT NOT NULL,
    seat        TEXT NOT NULL,
    handle      TEXT NOT NULL,
    session_id  TEXT,
    account_id  UUID REFERENCES accounts(id),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (match_id, seat)
);
CREATE INDEX IF NOT EXISTS match_participants_session_idx ON match_participants (session_id);
"#;

#[derive(Debug, thiserror::Error)]
pub enum PgError {
    #[error("postgres: {0}")]
    Pg(#[from] tokio_postgres::Error),
    #[error("handle already taken")]
    HandleTaken,
    #[error("sequence conflict: expected to write seq {expected}")]
    SeqConflict { expected: i64 },
}

pub struct Journal {
    client: Client,
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

impl Journal {
    /// Connect and migrate. The connection task is spawned onto tokio.
    pub async fn connect(url: &str) -> Result<Journal, PgError> {
        let (client, conn) = tokio_postgres::connect(url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::error!(error = %e, "postgres connection closed");
            }
        });
        client.batch_execute(SCHEMA).await?;
        Ok(Journal { client })
    }

    /// Record a usage/product event keyed by an anonymous session id (no PII).
    /// Best-effort analytics: callers log-and-continue on error — never block play.
    pub async fn record_usage(
        &self,
        session_id: Option<&str>,
        event: &str,
        match_id: Option<&str>,
    ) -> Result<(), PgError> {
        self.client
            .execute(
                "INSERT INTO usage_events (session_id, event, match_id) VALUES ($1, $2, $3)",
                &[&session_id, &event, &match_id],
            )
            .await?;
        Ok(())
    }

    /// Record who occupied a seat, name-tagged — anonymous and signed-in
    /// alike — so every journaled match carries the handle in use. Upserts on
    /// `(match_id, seat)`: a reconnect (a fresh `Hello` on the same seat) refreshes
    /// the handle/session/account rather than duplicating the row. `account_id` is
    /// `None` for an anonymous handle; it is the seam filled when an account
    /// claims a handle. Best-effort analytics, like [`record_usage`](Self::record_usage):
    /// callers log-and-continue on error — recording who played never blocks play.
    pub async fn record_participant(
        &self,
        match_id: &str,
        seat: &str,
        handle: &str,
        session_id: Option<&str>,
        account_id: Option<&str>,
    ) -> Result<(), PgError> {
        self.client
            .execute(
                // `$5::text::uuid` (not `$5::uuid`) so tokio-postgres binds the
                // nullable `&str` account id; the inner `::text` pins the type.
                "INSERT INTO match_participants (match_id, seat, handle, session_id, account_id)
                 VALUES ($1, $2, $3, $4, $5::text::uuid)
                 ON CONFLICT (match_id, seat) DO UPDATE
                 SET handle = EXCLUDED.handle,
                     session_id = EXCLUDED.session_id,
                     account_id = EXCLUDED.account_id",
                &[&match_id, &seat, &handle, &session_id, &account_id],
            )
            .await?;
        Ok(())
    }

    /// Create an account; returns (account_id, bearer_token). Only the HASH
    /// of the token is stored — a database read never yields a usable token.
    pub async fn create_account(&self, handle: &str) -> Result<(String, String), PgError> {
        use rand::TryRng;
        let mut raw = [0u8; 32];
        rand::rngs::SysRng
            .try_fill_bytes(&mut raw)
            .expect("OS CSPRNG unavailable");
        let token = hex::encode(raw);
        let row = self
            .client
            .query_one(
                "INSERT INTO accounts (handle, token_hash) VALUES ($1, $2)
                 ON CONFLICT (handle) DO NOTHING RETURNING id::text",
                &[&handle, &hash_token(&token)],
            )
            .await
            .map_err(|_| PgError::HandleTaken)?;
        Ok((row.get(0), token))
    }

    /// Token -> account id, constant-shape lookup by hash.
    pub async fn verify_token(&self, token: &str) -> Result<Option<String>, PgError> {
        let rows = self
            .client
            .query(
                "SELECT id::text FROM accounts WHERE token_hash = $1",
                &[&hash_token(token)],
            )
            .await?;
        Ok(rows.first().map(|r| r.get(0)))
    }

    pub async fn create_match(
        &self,
        match_id: &str,
        seed: i64,
        account_a: Option<&str>,
        account_b: Option<&str>,
    ) -> Result<(), PgError> {
        self.client
            .execute(
                // `$n::text::uuid`, not `$n::uuid`: a bare `$n::uuid` makes
                // Postgres infer the param's type as `uuid`, and tokio-postgres
                // then refuses to bind a `&str`. The inner `::text` pins it.
                "INSERT INTO matches (id, seed, account_a, account_b)
                 VALUES ($1::text::uuid, $2, $3::text::uuid, $4::text::uuid) ON CONFLICT DO NOTHING",
                &[&match_id, &seed, &account_a, &account_b],
            )
            .await?;
        Ok(())
    }

    /// Append one command's event batch in a single transaction. The primary
    /// key (match_id, seq) makes duplicate appends fail loudly, never silently.
    pub async fn append(
        &mut self,
        match_id: &str,
        seq: i64,
        payload: &[u8],
        draws_after: i64,
        schema_version: i32,
    ) -> Result<(), PgError> {
        let tx = self.client.transaction().await?;
        let n = tx
            .execute(
                "INSERT INTO match_events (match_id, seq, payload, draws_after, schema_version)
                 VALUES ($1::text::uuid, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                &[&match_id, &seq, &payload, &draws_after, &schema_version],
            )
            .await?;
        if n == 0 {
            tx.rollback().await?;
            return Err(PgError::SeqConflict { expected: seq });
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load(&self, match_id: &str) -> Result<Vec<(i64, Vec<u8>, i64)>, PgError> {
        let rows = self
            .client
            .query(
                "SELECT seq, payload, draws_after FROM match_events
                 WHERE match_id = $1::text::uuid ORDER BY seq",
                &[&match_id],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }

    pub async fn finish_match(&self, match_id: &str, result: &str) -> Result<(), PgError> {
        self.client
            .execute(
                "UPDATE matches SET result = $2 WHERE id = $1::text::uuid",
                &[&match_id, &result],
            )
            .await?;
        Ok(())
    }

    /// Recovery: record what a server restart needs to rebuild a live match —
    /// the API handle, the journal stream id, the seed, the seat-token HASHES, and
    /// the commit–reveal salt.
    ///
    /// `token_a_hash`/`token_b_hash` are `sha256(token)` (the server holds
    /// only the hash — `SeatToken` — and passes the digest here), so a database
    /// read never yields a usable seat credential. `seed_salt` is the commitment
    /// salt: persisting it lets recovery re-commit under the ORIGINAL salt, so the
    /// commitment published at creation can still be honoured after a restart.
    pub async fn register_match(
        &self,
        api_id: i64,
        db_id: &str,
        seed: i64,
        token_a_hash: &[u8],
        token_b_hash: &[u8],
        seed_salt: &[u8],
        // `Some(difficulty)` ⇒ Seat B is the server's bot; recovery re-attaches it.
        bot_difficulty: Option<&str>,
    ) -> Result<(), PgError> {
        self.client
            .execute(
                "INSERT INTO match_registry
                   (api_id, db_id, seed, mode, token_a_hash, token_b_hash, seed_salt, bot_difficulty)
                 VALUES ($1, $2, $3, '1v1', $4, $5, $6, $7) ON CONFLICT (api_id) DO NOTHING",
                &[
                    &api_id,
                    &db_id,
                    &seed,
                    &token_a_hash,
                    &token_b_hash,
                    &seed_salt,
                    &bot_difficulty,
                ],
            )
            .await?;
        Ok(())
    }

    /// 2v2: register a four-slot lobby. `slot_token_hashes` is the SHA-256
    /// digest of A1, B1, A2, B2 — the same order the server mints them; recovery
    /// rebuilds the lobby from these (hashes, never plaintext). `seed_salt`
    /// is the commit–reveal salt, persisted so recovery honours the original
    /// commitment.
    pub async fn register_match_2v2(
        &self,
        api_id: i64,
        db_id: &str,
        seed: i64,
        slot_token_hashes: &[&[u8]; 4],
        seed_salt: &[u8],
    ) -> Result<(), PgError> {
        self.client
            .execute(
                "INSERT INTO match_registry
                   (api_id, db_id, seed, mode, token_a_hash, token_b_hash, token_a2_hash, token_b2_hash, seed_salt)
                 VALUES ($1, $2, $3, '2v2', $4, $5, $6, $7, $8) ON CONFLICT (api_id) DO NOTHING",
                &[
                    &api_id,
                    &db_id,
                    &seed,
                    &slot_token_hashes[0],
                    &slot_token_hashes[1],
                    &slot_token_hashes[2],
                    &slot_token_hashes[3],
                    &seed_salt,
                ],
            )
            .await?;
        Ok(())
    }

    /// Look a match up by its API handle for reconnect-after-restart.
    pub async fn lookup_match(&self, api_id: i64) -> Result<Option<RegisteredMatch>, PgError> {
        let row = self
            .client
            .query_opt(
                "SELECT db_id, seed, mode, token_a_hash, token_b_hash, token_a2_hash, token_b2_hash,
                        seed_salt, bot_difficulty
                 FROM match_registry WHERE api_id = $1",
                &[&api_id],
            )
            .await?;
        Ok(row.map(|r| RegisteredMatch {
            db_id: r.get(0),
            seed: r.get(1),
            mode: r.get(2),
            token_a_hash: r.get(3),
            token_b_hash: r.get(4),
            token_a2_hash: r.get(5),
            token_b2_hash: r.get(6),
            seed_salt: r.get(7),
            bot_difficulty: r.get(8),
        }))
    }

    /// The highest API id ever registered — the server seeds its id counter past
    /// this on boot so a restart never reissues a live match's handle.
    pub async fn max_api_id(&self) -> Result<i64, PgError> {
        let row = self
            .client
            .query_one("SELECT COALESCE(MAX(api_id), 0) FROM match_registry", &[])
            .await?;
        Ok(row.get(0))
    }
}

/// What `lookup_match` returns — enough to resume a match from its journal.
/// `mode` is "1v1" or "2v2"; for 2v2, `token_a2_hash`/`token_b2_hash` carry slots
/// A2/B2 (they are `None` for 1v1).
///
/// The token fields are SHA-256 DIGESTS, never plaintext — recovery
/// rebuilds each in-memory `SeatToken` from the digest (`SeatToken::from_hash`)
/// and authorises a presented token against it, so no clear-text seat credential
/// is reconstructed. `seed_salt` is the commit–reveal salt: recovery re-commits
/// under it so the originally-published commitment is still honoured.
#[derive(Debug, Clone)]
pub struct RegisteredMatch {
    pub db_id: String,
    pub seed: i64,
    pub mode: String,
    pub token_a_hash: Vec<u8>,
    pub token_b_hash: Vec<u8>,
    pub token_a2_hash: Option<Vec<u8>>,
    pub token_b2_hash: Option<Vec<u8>>,
    /// The commit–reveal salt, so recovery re-commits identically.
    pub seed_salt: Vec<u8>,
    /// `Some(difficulty)` ⇒ a vs-bot 1v1; recovery re-attaches the Seat-B bot.
    pub bot_difficulty: Option<String>,
}

#[cfg(test)]
mod integration {
    use super::*;
    /// Run with a live Postgres: `make up && make db-test`
    /// (PG_URL=postgres://recollect:recollect@localhost:5432/recollect)
    #[tokio::test]
    #[ignore = "requires postgres (make up && make db-test)"]
    async fn accounts_journal_roundtrip_and_seq_conflicts() {
        let url = std::env::var("PG_URL").expect("PG_URL");
        let mut j = Journal::connect(&url).await.unwrap();
        // Unique per run: this test's asserts (HandleTaken on a second create,
        // SeqConflict on a duplicate append) are non-idempotent, so a fixed
        // pid-derived handle flaked when a recycled PID met leftover rows.
        let handle = format!(
            "tester-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let (acct, token) = j.create_account(&handle).await.unwrap();
        assert_eq!(
            j.verify_token(&token).await.unwrap().as_deref(),
            Some(acct.as_str())
        );
        assert!(j.verify_token("not-a-token").await.unwrap().is_none());
        assert!(matches!(
            j.create_account(&handle).await,
            Err(PgError::HandleTaken)
        ));

        let mid = uuid_like(&handle);
        j.create_match(&mid, 42, Some(&acct), None).await.unwrap();
        j.append(&mid, 0, b"batch0", 3, 1).await.unwrap();
        j.append(&mid, 1, b"batch1", 5, 1).await.unwrap();
        assert!(matches!(
            j.append(&mid, 1, b"dup", 9, 1).await,
            Err(PgError::SeqConflict { .. })
        ));
        let loaded = j.load(&mid).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1].2, 5, "draws_after rides beside the payload");
    }

    /// Every match records who played, name-tagged — anonymous (a server
    /// fallback handle, no account) AND signed-in (the account claimed at create).
    /// A reconnect (a fresh `Hello` on the same seat) upserts rather than
    /// duplicating, and the `(match_id, seat)` key keeps the two seats distinct.
    #[tokio::test]
    #[ignore = "requires postgres (make up && make db-test)"]
    async fn match_participants_record_the_name_anon_and_signed_in() {
        let url = std::env::var("PG_URL").expect("PG_URL");
        let j = Journal::connect(&url).await.unwrap();
        let suffix = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mid = uuid_like(&format!("participants-{suffix}"));

        // Seat A is a signed-in player; Seat B is an anonymous handle (no account).
        let (acct, _token) = j.create_account(&format!("ari-{suffix}")).await.unwrap();
        j.record_participant(&mid, "A", "Ari", Some("sid-a"), Some(&acct))
            .await
            .unwrap();
        j.record_participant(&mid, "B", "guest-1f2e3d", Some("sid-b"), None)
            .await
            .unwrap();

        // Both seats are name-tagged; the anon seat carries a handle but no account.
        let row_a = j
            .client
            .query_one(
                "SELECT handle, session_id, account_id::text FROM match_participants
                 WHERE match_id = $1 AND seat = 'A'",
                &[&mid],
            )
            .await
            .unwrap();
        assert_eq!(row_a.get::<_, String>(0), "Ari");
        assert_eq!(row_a.get::<_, Option<String>>(1).as_deref(), Some("sid-a"));
        assert_eq!(
            row_a.get::<_, Option<String>>(2).as_deref(),
            Some(acct.as_str()),
            "the signed-in seat binds its account"
        );
        let row_b = j
            .client
            .query_one(
                "SELECT handle, account_id::text FROM match_participants
                 WHERE match_id = $1 AND seat = 'B'",
                &[&mid],
            )
            .await
            .unwrap();
        assert_eq!(row_b.get::<_, String>(0), "guest-1f2e3d");
        assert!(
            row_b.get::<_, Option<String>>(1).is_none(),
            "an anonymous handle has no account — the seam is filled on claim"
        );

        // A reconnect renames the seat (the client persisted a new handle): the
        // upsert refreshes the row, never duplicates it.
        j.record_participant(&mid, "B", "guest-renamed", Some("sid-b"), None)
            .await
            .unwrap();
        let count: i64 = j
            .client
            .query_one(
                "SELECT count(*) FROM match_participants WHERE match_id = $1",
                &[&mid],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, 2, "two seats, one row each — the reconnect upserted");
        let renamed: String = j
            .client
            .query_one(
                "SELECT handle FROM match_participants WHERE match_id = $1 AND seat = 'B'",
                &[&mid],
            )
            .await
            .unwrap()
            .get(0);
        assert_eq!(renamed, "guest-renamed", "the upsert refreshed the handle");
    }

    /// The registry round-trips, ignores a duplicate api_id, and reports the
    /// high-water id the server seeds its counter past on boot.
    ///
    /// The seat tokens are stored as SHA-256 DIGESTS and the commit–reveal
    /// salt is persisted. The asserts pin both: the stored hash equals the digest
    /// passed in (so recovery rebuilds the same `SeatToken`), a stranger's token
    /// hashes to a DIFFERENT digest (so it can't authorise), and the salt comes
    /// back byte-identical (so a recovered match re-commits under the original).
    #[tokio::test]
    #[ignore = "requires postgres (make up && make db-test)"]
    async fn match_registry_round_trips_for_restart_recovery() {
        let url = std::env::var("PG_URL").expect("PG_URL");
        let j = Journal::connect(&url).await.unwrap();
        let api_id = (std::process::id() as i64) << 8 | 0x5A;
        let db = uuid_like(&format!("reg-{api_id}"));
        let (ha, hb) = (hash_token("tok-a"), hash_token("tok-b"));
        let salt = vec![0xAB_u8; 16];
        j.register_match(api_id, &db, 1234, &ha, &hb, &salt, None)
            .await
            .unwrap();
        // ON CONFLICT DO NOTHING: a re-register keeps the original row.
        let other_salt = vec![0xCD_u8; 16];
        j.register_match(
            api_id,
            &db,
            9999,
            &hash_token("other-a"),
            &hash_token("other-b"),
            &other_salt,
            Some("hard"),
        )
        .await
        .unwrap();
        let got = j.lookup_match(api_id).await.unwrap().expect("registered");
        assert_eq!(got.db_id, db);
        assert_eq!(got.seed, 1234);
        assert_eq!(got.mode, "1v1");
        // The digest round-trips exactly (recovery rebuilds the same SeatToken)…
        assert_eq!(
            got.token_a_hash, ha,
            "the stored hash is the token's digest"
        );
        // …and it is NOT the plaintext, and a stranger's token hashes differently.
        assert_ne!(
            got.token_a_hash,
            b"tok-a".to_vec(),
            "the registry stores the hash, never the plaintext token"
        );
        assert_ne!(
            got.token_a_hash,
            hash_token("not-the-token"),
            "a stranger's token hashes to a different digest — it can't authorise"
        );
        assert_eq!(
            got.seed_salt, salt,
            "the original commit–reveal salt persists"
        );
        assert!(got.token_a2_hash.is_none(), "1v1 has no slot-A2 token");
        assert!(got.bot_difficulty.is_none(), "this match is human-vs-human");

        // A vs-bot registration round-trips its difficulty. (Distinct low byte so
        // it can't collide with the api_id / api_id+1 the asserts above reserve.)
        let bot_id = (std::process::id() as i64) << 8 | 0xB7;
        let bot_db = uuid_like(&format!("reg-bot-{bot_id}"));
        j.register_match(
            bot_id,
            &bot_db,
            77,
            &hash_token("a"),
            &hash_token("b"),
            &[0x11_u8; 16],
            Some("expert"),
        )
        .await
        .unwrap();
        assert_eq!(
            j.lookup_match(bot_id)
                .await
                .unwrap()
                .unwrap()
                .bot_difficulty
                .as_deref(),
            Some("expert")
        );
        assert!(j.lookup_match(api_id + 1).await.unwrap().is_none());
        assert!(
            j.max_api_id().await.unwrap() >= api_id,
            "high-water id seen"
        );
    }

    /// 2v2: the registry carries the mode tag and all four slot-token hashes
    /// plus the salt, so a restart can rebuild the lobby and honour the commitment.
    /// Each slot's stored value is the token's DIGEST, never the plaintext.
    #[tokio::test]
    #[ignore = "requires postgres (make up && make db-test)"]
    async fn match_registry_round_trips_a_2v2_lobby() {
        let url = std::env::var("PG_URL").expect("PG_URL");
        let j = Journal::connect(&url).await.unwrap();
        let api_id = (std::process::id() as i64) << 8 | 0x2B;
        let db = uuid_like(&format!("reg2v2-{api_id}"));
        let hashes = [
            hash_token("a1"),
            hash_token("b1"),
            hash_token("a2"),
            hash_token("b2"),
        ];
        let refs: [&[u8]; 4] = [&hashes[0], &hashes[1], &hashes[2], &hashes[3]];
        let salt = vec![0x5A_u8; 16];
        j.register_match_2v2(api_id, &db, 4242, &refs, &salt)
            .await
            .unwrap();
        let got = j.lookup_match(api_id).await.unwrap().expect("registered");
        assert_eq!(got.mode, "2v2");
        assert_eq!(got.seed, 4242);
        assert_eq!(got.token_a_hash, hash_token("a1"));
        assert_eq!(got.token_b_hash, hash_token("b1"));
        assert_eq!(got.token_a2_hash.as_deref(), Some(&hash_token("a2")[..]));
        assert_eq!(got.token_b2_hash.as_deref(), Some(&hash_token("b2")[..]));
        assert_ne!(
            got.token_a_hash,
            b"a1".to_vec(),
            "a slot token is stored hashed, never in the clear"
        );
        assert_eq!(got.seed_salt, salt, "the 2v2 commit–reveal salt persists");
    }

    fn uuid_like(seed: &str) -> String {
        let h = Sha256::digest(seed.as_bytes());
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:01x}{:02x}-8{:01x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            h[0],
            h[1],
            h[2],
            h[3],
            h[4],
            h[5],
            h[6] & 0xF,
            h[7],
            h[8] & 0xF,
            h[9],
            h[10],
            h[11],
            h[12],
            h[13],
            h[14],
            h[15]
        )
    }
}
