//! The ironstate event journal over Postgres — the **async, authoritative**
//! store. Production drives [`execute_async`] (append-before-ack via ironstate's
//! `prepare`/`commit`/`abort`); the seven-property *storage* proof lives in
//! `tests/journal_contract.rs`, a synchronous `Journal` twin over this very
//! schema, so "we went async" never means "we left the yardstick behind".
use std::marker::PhantomData;

use ironstate_aggregate::{Aggregate, AggregateRules, CtxEntropy, DrawPos};
use ironstate_journal::{
    ExecuteError, JournalError, ResumeError, Seq, Snapshot, VersionedEvent, prepare, replay,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio_postgres::Client;

/// The generic, FK-free journal schema (decoupled from accounts/matches). Both
/// the async store here and the contract test's sync twin drive these tables, so
/// proving the twin proves the storage for both.
///
/// The setup is bracketed by a session advisory lock: `CREATE TABLE IF NOT EXISTS`
/// does *not* take a lock strong enough to keep two sessions from racing into a
/// duplicate `pg_type` insert (E23505), so concurrent first-time initialization —
/// several test connections, or a horizontally-scaled startup — would otherwise
/// flake. The lock serializes that window; everyone after the winner sees the
/// tables already present and no-ops.
pub const JOURNAL_SCHEMA: &str = r#"
SELECT pg_advisory_lock(8765309);
CREATE TABLE IF NOT EXISTS journal_events (
    stream_id      TEXT   NOT NULL,
    seq            BIGINT NOT NULL,
    payload        BYTEA  NOT NULL,            -- postcard batch of A::Event
    entropy_pos    BIGINT NOT NULL,            -- the position the batch consumed
    schema_version INT    NOT NULL,
    PRIMARY KEY (stream_id, seq)               -- the optimistic-concurrency guard
);
CREATE TABLE IF NOT EXISTS journal_snapshots (
    stream_id      TEXT   NOT NULL,
    at_seq         BIGINT NOT NULL,
    entropy_pos    BIGINT NOT NULL,
    schema_version INT    NOT NULL,
    state          BYTEA  NOT NULL,            -- postcard A
    PRIMARY KEY (stream_id, at_seq)
);
SELECT pg_advisory_unlock(8765309);
"#;

fn store_err<E: std::error::Error + Send + Sync + 'static>(e: E) -> JournalError {
    JournalError::Storage(Box::new(e))
}

/// One aggregate stream (e.g. a match) over Postgres, reached through an async
/// client. Its operations are `async fn`, which is exactly why it can't be
/// ironstate's synchronous `Journal`; [`execute_async`] supplies the missing
/// discipline through `prepare`/`Prepared`.
pub struct AsyncStore<'c, A> {
    client: &'c Client,
    stream: String,
    _a: PhantomData<A>,
}

impl<'c, A> AsyncStore<'c, A>
where
    A: AggregateRules + Clone + Serialize + DeserializeOwned,
    A::Event: Serialize + DeserializeOwned,
{
    /// Bind to an aggregate stream on an existing connection, writing the genesis
    /// snapshot if the stream is new (a no-op on reopen). `genesis_pos` is the
    /// entropy position the genesis state already sits at — `DrawPos(0)` for a
    /// fresh aggregate, but recollect's match opening consumes draws (shuffles,
    /// stray seeding), so a resume with zero journaled commands must reposition
    /// the stream there, not at zero.
    pub async fn open(
        client: &'c Client,
        stream: impl Into<String>,
        genesis: &A,
        genesis_pos: DrawPos,
    ) -> Result<Self, JournalError> {
        let stream = stream.into();
        let state = postcard::to_allocvec(genesis).map_err(store_err)?;
        client
            .execute(
                "INSERT INTO journal_snapshots (stream_id, at_seq, entropy_pos, schema_version, state)
                 VALUES ($1, 0, $2, 0, $3) ON CONFLICT DO NOTHING",
                &[&stream, &(genesis_pos.0 as i64), &state],
            )
            .await
            .map_err(store_err)?;
        Ok(Self {
            client,
            stream,
            _a: PhantomData,
        })
    }

    /// Bind to an existing stream without touching the genesis snapshot — the
    /// per-command handle (the stream was created by [`open`](Self::open) at match
    /// start). Pure; does no IO.
    pub fn attach(client: &'c Client, stream: impl Into<String>) -> Self {
        Self {
            client,
            stream: stream.into(),
            _a: PhantomData,
        }
    }

    pub async fn head(&self) -> Result<Option<Seq>, JournalError> {
        let row = self
            .client
            .query_one(
                "SELECT MAX(seq) FROM journal_events WHERE stream_id = $1",
                &[&self.stream],
            )
            .await
            .map_err(store_err)?;
        Ok(row.get::<_, Option<i64>>(0).map(|m| Seq(m as u64)))
    }

    pub async fn entropy_pos(&self, at: Seq) -> Result<DrawPos, JournalError> {
        if at.0 == 0 {
            let row = self
                .client
                .query_opt(
                    "SELECT entropy_pos FROM journal_snapshots WHERE stream_id = $1 AND at_seq = 0",
                    &[&self.stream],
                )
                .await
                .map_err(store_err)?;
            return Ok(row.map_or(DrawPos(0), |r| DrawPos(r.get::<_, i64>(0) as u64)));
        }
        let row = self
            .client
            .query_opt(
                "SELECT entropy_pos FROM journal_events WHERE stream_id = $1 AND seq = $2",
                &[&self.stream, &(at.0 as i64)],
            )
            .await
            .map_err(store_err)?;
        row.map(|r| DrawPos(r.get::<_, i64>(0) as u64))
            .ok_or(JournalError::UnknownSeq { at })
    }

    /// The entropy position at the head — what `execute_async` reads before
    /// deciding (`DrawPos(0)` for an empty stream).
    pub async fn head_pos(&self) -> Result<DrawPos, JournalError> {
        match self.head().await? {
            Some(h) => self.entropy_pos(h).await,
            None => Ok(DrawPos(0)),
        }
    }

    /// Append one batch and its entropy position. `next = head + 1`: recollect is
    /// single-writer per stream (the server owns each match), so this is
    /// contention-free; the `(stream, seq)` primary key is the backstop that turns
    /// an accidental double-append into a loud error, never a silent duplicate. On
    /// any append error `execute_async` aborts, rewinding the entropy stream.
    pub async fn append(
        &self,
        events: &[A::Event],
        entropy_pos: DrawPos,
    ) -> Result<Seq, JournalError> {
        let next = self.head().await?.map_or(1, |s| s.0 + 1);
        let payload = postcard::to_allocvec(&events.to_vec()).map_err(store_err)?;
        let n = self
            .client
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
            .await
            .map_err(store_err)?;
        if n == 0 {
            return Err(JournalError::Storage(
                "seq conflict: a concurrent append moved the head".into(),
            ));
        }
        Ok(Seq(next))
    }

    pub async fn events_since(
        &self,
        after: Option<Seq>,
    ) -> Result<Vec<VersionedEvent<A>>, JournalError> {
        let from = after.map_or(0, |s| s.0) as i64;
        let rows = self
            .client
            .query(
                "SELECT payload FROM journal_events WHERE stream_id = $1 AND seq > $2 ORDER BY seq",
                &[&self.stream, &from],
            )
            .await
            .map_err(store_err)?;
        let type_name = std::borrow::Cow::Borrowed(std::any::type_name::<A::Event>());
        let mut out = Vec::new();
        for r in rows {
            let payload: Vec<u8> = r.get(0);
            let batch: Vec<A::Event> = postcard::from_bytes(&payload).map_err(store_err)?;
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

    pub async fn latest_snapshot(&self) -> Result<Option<Snapshot<A>>, JournalError> {
        let row = self
            .client
            .query_opt(
                "SELECT at_seq, entropy_pos, schema_version, state FROM journal_snapshots
                 WHERE stream_id = $1 ORDER BY at_seq DESC LIMIT 1",
                &[&self.stream],
            )
            .await
            .map_err(store_err)?;
        match row {
            None => Ok(None),
            Some(r) => {
                let state: Vec<u8> = r.get(3);
                let state: A = postcard::from_bytes(&state).map_err(store_err)?;
                Ok(Some(Snapshot {
                    state,
                    schema_version: r.get::<_, i32>(2) as u32,
                    at: Seq(r.get::<_, i64>(0) as u64),
                    entropy_pos: DrawPos(r.get::<_, i64>(1) as u64),
                }))
            }
        }
    }
}

/// The persistent loop: read head → `prepare` → append (the one mutating await)
/// → `commit`/`abort`. recollect owns only the IO; ironstate's `prepare` /
/// `Prepared` carry the entropy-position capture, the append-before-evolve
/// ordering, and the rewind — so this can't drift from the in-memory `execute`.
pub async fn execute_async<A>(
    store: &AsyncStore<'_, A>,
    aggregate: &mut Aggregate<A>,
    cmd: &A::Command,
    ctx: &mut A::Ctx,
) -> Result<Seq, ExecuteError<A>>
where
    A: AggregateRules + Clone + Serialize + DeserializeOwned,
    A::Event: Serialize + DeserializeOwned,
    A::Ctx: CtxEntropy,
{
    let head = store.head_pos().await.map_err(ExecuteError::Journal)?;
    let prepared = prepare(aggregate, cmd, ctx, head).map_err(ExecuteError::Rejected)?;
    match store
        .append(prepared.events(), prepared.entropy_pos())
        .await
    {
        Ok(seq) => {
            prepared.commit(aggregate);
            Ok(seq)
        }
        Err(error) => {
            prepared.abort(ctx);
            Err(ExecuteError::Journal(error))
        }
    }
}

/// Rebuild an aggregate from the store on restart — `resume`, async. Returns the
/// aggregate and the entropy position recorded **at the head** (the authoritative
/// one, not the snapshot's), so the caller repositions its live stream there.
pub async fn resume_async<A>(
    store: &AsyncStore<'_, A>,
) -> Result<(Aggregate<A>, DrawPos), ResumeError>
where
    A: AggregateRules + Clone + Serialize + DeserializeOwned,
    A::Event: Serialize + DeserializeOwned,
{
    let snapshot = store
        .latest_snapshot()
        .await
        .map_err(ResumeError::Journal)?
        .ok_or(ResumeError::NoBase)?;
    let snapshot_pos = snapshot.entropy_pos;
    let from = snapshot.at;
    let events = store
        .events_since(Some(from))
        .await
        .map_err(ResumeError::Journal)?;
    let aggregate = replay(snapshot, &events).map_err(ResumeError::Restore)?;
    let pos = match store.head().await.map_err(ResumeError::Journal)? {
        Some(h) => store.entropy_pos(h).await.map_err(ResumeError::Journal)?,
        None => snapshot_pos,
    };
    Ok((aggregate, pos))
}
