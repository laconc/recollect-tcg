//! The seven-property journal contract, run against the **real Postgres
//! storage** through a synchronous `Journal` twin.
//!
//! Production drives this same SQL over an async client (the `execute_async`
//! loop using ironstate's `prepare`/`commit`/`abort`); this test exists so the
//! durable storage — the only place a hand-rolled async loop can corrupt
//! durability — is held to ironstate's `journal_contract_test!` yardstick:
//! position totality/monotonicity, failed-append atomicity, snapshot-vs-head,
//! and fork-position equality. The async front end inherits the proof because it
//! drives the identical schema.
//!
//! Requires Postgres (an `--ignored` test, like the rest of the db suite):
//! `make up && PG_URL=… cargo test -p recollect-journal-postgres -- --ignored`.
use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use ironstate_aggregate::{AggregateRules, DrawPos};
use ironstate_journal::testkit_support::run_contract;
use ironstate_journal::{ContractJournal, Journal, JournalError, Seq, Snapshot, VersionedEvent};
use recollect_journal_postgres::store::JOURNAL_SCHEMA;
use serde::Serialize;
use serde::de::DeserializeOwned;

// The aggregate under test and the schema both live elsewhere so the contract's
// SYNC twin and production's async `AsyncStore` measure the *same* storage:
// `Tally` in `tests/common`, `JOURNAL_SCHEMA` in `src/store.rs`.
mod common;
use common::Tally;

fn pg_err<E: std::error::Error + Send + Sync + 'static>(e: E) -> JournalError {
    JournalError::Storage(Box::new(e))
}

fn connect() -> postgres::Client {
    let url = std::env::var("PG_URL").expect("PG_URL for the journal contract test");
    let mut client = postgres::Client::connect(&url, postgres::NoTls).expect("connect");
    client.batch_execute(JOURNAL_SCHEMA).expect("schema");
    client
}

static STREAM_SEQ: AtomicU64 = AtomicU64::new(0);
fn next_stream() -> String {
    format!(
        "contract-{}-{}",
        std::process::id(),
        STREAM_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// A synchronous [`Journal`] over Postgres — the contract's measuring stick. Maps
/// each method to one statement against the schema above. The sync `postgres`
/// client needs `&mut self` for queries while `Journal`'s reads are `&self`, so
/// the client sits behind a `RefCell` (the contract harness is single-threaded).
struct PgStore<A> {
    client: RefCell<postgres::Client>,
    stream: String,
    _a: PhantomData<A>,
}

impl<A> Journal<A> for PgStore<A>
where
    A: AggregateRules + Clone + Serialize + DeserializeOwned,
    A::Event: Serialize + DeserializeOwned,
{
    fn append(&mut self, events: &[A::Event], entropy_pos: DrawPos) -> Result<Seq, JournalError> {
        let next = self.head().map_or(1, |s| s.0 + 1);
        let payload = postcard::to_allocvec(&events.to_vec()).map_err(pg_err)?;
        let n = self
            .client
            .borrow_mut()
            .execute(
                "INSERT INTO journal_events (stream_id, seq, payload, entropy_pos, schema_version)
             VALUES ($1, $2, $3, $4, 1) ON CONFLICT DO NOTHING",
                &[
                    &self.stream,
                    &(next as i64),
                    &payload,
                    &(entropy_pos.0 as i64),
                ],
            )
            .map_err(pg_err)?;
        if n == 0 {
            return Err(JournalError::Storage("seq conflict: head moved".into()));
        }
        Ok(Seq(next))
    }

    fn entropy_pos(&self, at: Seq) -> Result<DrawPos, JournalError> {
        if at.0 == 0 {
            let row = self
                .client
                .borrow_mut()
                .query_opt(
                    "SELECT entropy_pos FROM journal_snapshots WHERE stream_id = $1 AND at_seq = 0",
                    &[&self.stream],
                )
                .map_err(pg_err)?;
            return Ok(row.map_or(DrawPos(0), |r| DrawPos(r.get::<_, i64>(0) as u64)));
        }
        let row = self
            .client
            .borrow_mut()
            .query_opt(
                "SELECT entropy_pos FROM journal_events WHERE stream_id = $1 AND seq = $2",
                &[&self.stream, &(at.0 as i64)],
            )
            .map_err(pg_err)?;
        row.map(|r| DrawPos(r.get::<_, i64>(0) as u64))
            .ok_or(JournalError::UnknownSeq { at })
    }

    fn head(&self) -> Option<Seq> {
        let row = self
            .client
            .borrow_mut()
            .query_one(
                "SELECT MAX(seq) FROM journal_events WHERE stream_id = $1",
                &[&self.stream],
            )
            .ok()?;
        row.get::<_, Option<i64>>(0).map(|m| Seq(m as u64))
    }

    fn events_since(&self, after: Option<Seq>) -> Result<Vec<VersionedEvent<A>>, JournalError> {
        let from = after.map_or(0, |s| s.0) as i64;
        let rows = self
            .client
            .borrow_mut()
            .query(
                "SELECT payload FROM journal_events WHERE stream_id = $1 AND seq > $2 ORDER BY seq",
                &[&self.stream, &from],
            )
            .map_err(pg_err)?;
        let type_name = std::borrow::Cow::Borrowed(std::any::type_name::<A::Event>());
        let mut out = Vec::new();
        for r in rows {
            let payload: Vec<u8> = r.get(0);
            let batch: Vec<A::Event> = postcard::from_bytes(&payload).map_err(pg_err)?;
            for event in batch {
                out.push(VersionedEvent {
                    event,
                    type_name: type_name.clone(),
                    version: 1,
                });
            }
        }
        Ok(out)
    }

    fn snapshot(&mut self, snapshot: Snapshot<A>) -> Result<(), JournalError> {
        let state = postcard::to_allocvec(&snapshot.state).map_err(pg_err)?;
        self.client.borrow_mut().execute(
            "INSERT INTO journal_snapshots (stream_id, at_seq, entropy_pos, schema_version, state)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (stream_id, at_seq)
             DO UPDATE SET entropy_pos = EXCLUDED.entropy_pos, state = EXCLUDED.state",
            &[
                &self.stream,
                &(snapshot.at.0 as i64),
                &(snapshot.entropy_pos.0 as i64),
                &(snapshot.schema_version as i32),
                &state,
            ],
        ).map_err(pg_err)?;
        Ok(())
    }

    fn latest_snapshot(&self) -> Result<Option<Snapshot<A>>, JournalError> {
        let row = self
            .client
            .borrow_mut()
            .query_opt(
                "SELECT at_seq, entropy_pos, schema_version, state FROM journal_snapshots
             WHERE stream_id = $1 ORDER BY at_seq DESC LIMIT 1",
                &[&self.stream],
            )
            .map_err(pg_err)?;
        match row {
            None => Ok(None),
            Some(r) => {
                let state: Vec<u8> = r.get(3);
                let state: A = postcard::from_bytes(&state).map_err(pg_err)?;
                Ok(Some(Snapshot {
                    state,
                    schema_version: r.get::<_, i32>(2) as u32,
                    at: Seq(r.get::<_, i64>(0) as u64),
                    entropy_pos: DrawPos(r.get::<_, i64>(1) as u64),
                }))
            }
        }
    }

    fn fork(&self, at: Seq) -> Result<Self, JournalError> {
        if at.0 > self.head().map_or(0, |s| s.0) {
            return Err(JournalError::UnknownSeq { at });
        }
        let mut client = connect();
        let stream = next_stream();
        let at = at.0 as i64;
        client
            .execute(
                "INSERT INTO journal_events (stream_id, seq, payload, entropy_pos, schema_version)
             SELECT $1, seq, payload, entropy_pos, schema_version FROM journal_events
             WHERE stream_id = $2 AND seq <= $3",
                &[&stream, &self.stream, &at],
            )
            .map_err(pg_err)?;
        client.execute(
            "INSERT INTO journal_snapshots (stream_id, at_seq, entropy_pos, schema_version, state)
             SELECT $1, at_seq, entropy_pos, schema_version, state FROM journal_snapshots
             WHERE stream_id = $2 AND at_seq <= $3",
            &[&stream, &self.stream, &at],
        ).map_err(pg_err)?;
        Ok(PgStore {
            client: RefCell::new(client),
            stream,
            _a: PhantomData,
        })
    }
}

impl<A> ContractJournal<A> for PgStore<A>
where
    A: AggregateRules + Clone + Serialize + DeserializeOwned,
    A::Event: Serialize + DeserializeOwned,
{
    fn fresh(genesis: A) -> Self {
        let mut client = connect();
        let stream = next_stream();
        let state = postcard::to_allocvec(&genesis).expect("serialize genesis");
        client
            .execute(
                "INSERT INTO journal_snapshots (stream_id, at_seq, entropy_pos, schema_version, state)
                 VALUES ($1, 0, 0, 0, $2)",
                &[&stream, &state],
            )
            .expect("seed genesis snapshot");
        PgStore {
            client: RefCell::new(client),
            stream,
            _a: PhantomData,
        }
    }
}

/// The durable Postgres storage passes the same seven-property journal contract
/// `MemoryJournal` meets — even though production drives it over an async client.
#[test]
#[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-journal-postgres -- --ignored)"]
fn pg_storage_meets_the_journal_contract() {
    run_contract::<PgStore<Tally>, Tally>(64, 24, 0xC047);
}
