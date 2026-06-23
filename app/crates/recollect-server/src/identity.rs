//! The launch identity model — server side (companion to
//! `docs/decisions/playtest_launch_plan.md` §1).
//!
//! At launch there are **no accounts**. A player is an
//! **anonymous identity**: a generated handle. The *client* mints and persists the
//! real handle (a `localStorage` value on the web, a separate lane) and sends it on
//! the [`recollect_protocol::ClientMsg::Hello`]; the **server only records it**, so
//! every journaled match is name-tagged from day one. This module owns the two
//! server-side rules that keeps that recording honest and forward-compatible:
//!
//! 1. [`participant_handle`] — resolve the handle to record for a seat from the
//!    optional `Hello.name`. A present, non-blank name is clamped to a sane length
//!    and recorded verbatim; an absent/blank one falls back to a minimal generated
//!    handle ([`fallback_handle`]) so the match is never name-less. (When a UI ever
//!    renders a handle it MUST use `textContent` — a handle is untrusted text.)
//! 2. Forward-compatibility with accounts: "who played" is a **handle**, and
//!    an account later *claims* a handle. The journal's `match_participants` row
//!    carries a nullable `account_id` for exactly that — `None` here at launch.
//!
//! This is deliberately tiny and account-free: play is anonymous, and the nullable
//! `account_id` seam keeps it forward-compatible with a durable account layer (OIDC,
//! claimable handles) without coupling to one (launch-plan §1).

use rand::TryRng;

/// The longest handle the server stores. Matches the display-name clamp the actor
/// applies before echoing a name to the table (`actor.rs`), so the recorded handle
/// and the rendered one agree.
pub(crate) const MAX_HANDLE_LEN: usize = 40;

/// A minimal server-side fallback handle for a seat whose `Hello` carried no name —
/// `guest-<6 hex>` from the OS CSPRNG. It keeps the match journaling complete
/// (every seat name-tagged) without pretending to be a durable identity: the real,
/// persisted handle is the client's to mint (the web lane). Anonymous by nature —
/// no account, no PII, not derived from any client value.
pub(crate) fn fallback_handle() -> String {
    let mut b = [0u8; 3];
    rand::rngs::SysRng
        .try_fill_bytes(&mut b)
        .expect("OS CSPRNG unavailable");
    format!("guest-{}", hex::encode(b))
}

/// Resolve the handle to record for a seat from the optional `Hello.name`. A
/// present, non-blank name is trimmed, clamped to [`MAX_HANDLE_LEN`], and recorded
/// as-is (the client owns generating + persisting it); anything absent or blank
/// falls back to [`fallback_handle`] so no journaled match is ever name-less.
pub(crate) fn participant_handle(name: Option<&str>) -> String {
    match name.map(str::trim) {
        Some(n) if !n.is_empty() => n.chars().take(MAX_HANDLE_LEN).collect(),
        _ => fallback_handle(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A present name is recorded verbatim (the client owns it); a blank or absent
    /// one falls back to a generated anonymous handle so the seat is still tagged.
    #[test]
    fn a_present_name_is_kept_and_a_missing_one_falls_back() {
        assert_eq!(participant_handle(Some("Ari")), "Ari");
        // Absent ⇒ a generated guest handle (never empty).
        let anon = participant_handle(None);
        assert!(
            anon.starts_with("guest-"),
            "fallback is a guest handle: {anon}"
        );
        assert!(
            participant_handle(Some("   ")).starts_with("guest-"),
            "blank falls back too"
        );
    }

    /// An over-long handle is clamped (untrusted client text), matching the actor's
    /// display-name clamp so the recorded and rendered handles agree.
    #[test]
    fn an_over_long_handle_is_clamped() {
        let long = "x".repeat(200);
        assert_eq!(
            participant_handle(Some(&long)).chars().count(),
            MAX_HANDLE_LEN
        );
    }

    /// Fallback handles are anonymous and effectively unique — generated from OS
    /// entropy, not derived from any client value.
    #[test]
    fn fallback_handles_are_distinct() {
        let a: std::collections::HashSet<String> = (0..64).map(|_| fallback_handle()).collect();
        assert!(
            a.len() >= 63,
            "guest handles collide far too often: {}",
            a.len()
        );
    }
}
