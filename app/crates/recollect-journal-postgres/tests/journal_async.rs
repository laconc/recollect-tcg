//! The production async loop against **live Postgres**: `execute_async` (append-
//! before-ack) and `resume_async` (rebuild from the head). The seven storage
//! properties are proven separately by `journal_contract.rs` over the same schema;
//! these tests are the end-to-end check that the hand-rolled async loop preserves
//! the entropy-position discipline a live run depends on.
//!
//! Requires Postgres (an `--ignored` test, like the rest of the db suite):
//! `make up && PG_URL=… cargo test -p recollect-journal-postgres -- --ignored`.
use std::sync::atomic::{AtomicU64, Ordering};

use ironstate_aggregate::{
    Aggregate, DrawPos, LogicalTime, OwnedDeterministicCtx, Seed, SeededEntropy,
};
use recollect_journal_postgres::store::{AsyncStore, JOURNAL_SCHEMA, execute_async, resume_async};
use tokio_postgres::{Client, NoTls};

mod common;
use common::{Command, Tally, genesis};

async fn connect() -> Client {
    let url = std::env::var("PG_URL").expect("PG_URL for the async journal tests");
    let (client, conn) = tokio_postgres::connect(&url, NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    client.batch_execute(JOURNAL_SCHEMA).await.expect("schema");
    client
}

static STREAM_SEQ: AtomicU64 = AtomicU64::new(0);
fn next_stream() -> String {
    format!(
        "async-{}-{}",
        std::process::id(),
        STREAM_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// A context whose entropy sits at the store's head — what a live stream expects
/// before each command (`prepare` draws from the current position; `head` is only
/// the rewind target).
async fn ctx_at_head(store: &AsyncStore<'_, Tally>, seed: &Seed) -> OwnedDeterministicCtx<u32> {
    let pos = store.head_pos().await.expect("head_pos");
    OwnedDeterministicCtx {
        entropy: Box::new(SeededEntropy::at(seed, pos)),
        actor: 0,
        now: LogicalTime(0),
    }
}

/// Six rolls through `execute_async`, then a fresh resume reproduces the live
/// state exactly — with the entropy repositioned at the **head**, not the genesis
/// snapshot. That position equality is the proof the entropy-position discipline
/// survived the hand-rolled async loop (a resume off the snapshot would replay the
/// draws from the wrong offset and silently diverge).
#[tokio::test]
#[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-journal-postgres -- --ignored)"]
async fn execute_async_is_durable_and_resume_matches_a_live_run() {
    let seed = Seed([42u8; 32]);
    let client = connect().await;
    let store = AsyncStore::open(&client, next_stream(), &genesis(), DrawPos(0))
        .await
        .unwrap();
    let mut live = Aggregate::new(genesis()).unwrap();

    for _ in 0..6 {
        let mut ctx = ctx_at_head(&store, &seed).await;
        execute_async(&store, &mut live, &Command::Roll, &mut ctx)
            .await
            .unwrap();
    }
    assert_eq!(
        store.head().await.unwrap().map(|s| s.0),
        Some(6),
        "every command appended before it was acked"
    );
    assert!((6..=36).contains(&live.state().total), "six d6 rolls");

    let (resumed, pos) = resume_async::<Tally>(&store).await.unwrap();
    assert_eq!(
        resumed.state(),
        live.state(),
        "resume reproduces live state"
    );
    assert_eq!(
        pos,
        store.head_pos().await.unwrap(),
        "resume sits at the head, not the snapshot"
    );
}

/// A stream with only its genesis snapshot resumes to the seed state at position
/// zero — the empty-head branch of `resume_async`/`head_pos`.
#[tokio::test]
#[ignore = "requires postgres (make up && PG_URL=… cargo test -p recollect-journal-postgres -- --ignored)"]
async fn resume_from_genesis_only_yields_the_seed_state() {
    let client = connect().await;
    let store = AsyncStore::open(&client, next_stream(), &genesis(), DrawPos(0))
        .await
        .unwrap();
    let (resumed, pos) = resume_async::<Tally>(&store).await.unwrap();
    assert_eq!(resumed.state(), &genesis());
    assert_eq!(pos, DrawPos(0));
}
