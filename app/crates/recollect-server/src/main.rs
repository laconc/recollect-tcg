//! The `recollect-server` binary — a thin wrapper over [`recollect_server::run`].
//! All the logic lives in the library so integration tests (and the bundled CLI's
//! round-trip test) can drive the same routes in-process.
#![forbid(unsafe_code)]

#[tokio::main]
async fn main() {
    recollect_server::run().await;
}
