//! Match-seed security: CSPRNG seeds, hashed seat tokens, and a commit–reveal
//! over the seed. Everything a non-localhost PvP deployment needs so neither the
//! server nor a player can rig or grind a match.
//!
//! Three independent pieces, all rooted in the OS CSPRNG ([`rand::rngs::SysRng`]):
//!
//! 1. **Seeds come from the OS CSPRNG** ([`fresh_seed`]) — never `SystemTime`,
//!    which a watcher can predict to the nanosecond and grind. The seed
//!    appears in NO event and NO [`recollect_core::view::PlayerView`] (the
//!    determinism + redaction invariants hold — only its *generation* is
//!    hardened); `?seed=` stays as the explicit debug/repro override.
//! 2. **Seat tokens are stored hashed** ([`SeatToken`]) — the plaintext is
//!    minted from OS entropy and handed to the client exactly once (the POST
//!    response); the live [`super::Match`]'s [`super::MatchKind`] holds only
//!    its SHA-256 digest, and a presented token is authorised by hashing it and
//!    comparing in constant time. An in-memory dump (or a stray struct echo)
//!    never yields a usable credential. The Postgres `match_registry` persists the
//!    SAME digest, never the plaintext, so a database read can't leak a
//!    live seat credential either — [`SeatToken::from_hash`] rebuilds the handle
//!    from the stored digest on recovery. Mirrors the account-token discipline in
//!    `recollect-journal-postgres`.
//! 3. **Commit–reveal over the seed** ([`SeedCommitment`]) — at match creation
//!    the server publishes `commit = SHA-256(seed_le ‖ salt)` (a random 16-byte
//!    salt) and keeps the secret `seed`/`salt`; at match end it reveals both, so
//!    anyone can recompute the commitment, confirm it matches what was published
//!    *before* a single card moved, then replay the command log against that
//!    seed — a provably-fair shuffle no side could have rigged after the fact.
//!    The salt makes the commitment hiding even though the seed space is small.
//!    The salt is persisted with the match (the `match_registry`
//!    `seed_salt` column), so a match recovered after a restart re-commits under
//!    the ORIGINAL salt ([`SeedCommitment::from_parts`]) — the published
//!    commitment stays honourable across a crash, not only within one process.

use rand::TryRng;
use sha2::{Digest, Sha256};

/// A fresh 64-bit match seed from the OS CSPRNG. The deterministic core consumes
/// it exactly as before — only the *source* hardened (was `SystemTime`, which is
/// predictable and grindable). The seed never reaches an event or a view.
pub(crate) fn fresh_seed() -> u64 {
    let mut b = [0u8; 8];
    rand::rngs::SysRng
        .try_fill_bytes(&mut b)
        .expect("OS CSPRNG unavailable");
    u64::from_le_bytes(b)
}

/// A constant-time byte-slice equality (length-independent only for equal
/// lengths — digests here are always 32 bytes). Comparing SHA-256 *digests*
/// leaks nothing about the token even in variable time (preimage resistance),
/// but a non-short-circuiting compare is the correct, cheap habit for anything
/// gating authorisation.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A short-lived per-seat credential, held only as its SHA-256 hash.
///
/// [`SeatToken::mint`] returns `(SeatToken, plaintext)`: hand the plaintext to
/// the client once (it can never be recovered from the stored hash), keep the
/// `SeatToken` on the match. [`SeatToken::matches`] authorises a presented
/// plaintext by hashing it and comparing the digests in constant time.
///
/// `recover_match` reconstructs a token from the digest the registry persists
/// ([`SeatToken::from_hash`]) — the Postgres `match_registry` stores the SHA-256
/// digest, never the plaintext (that schema lives in
/// `recollect-journal-postgres`, outside this crate). So neither the running
/// server NOR the database keeps a live seat credential in the clear: a DB read
/// yields the same hash the in-memory handle holds, and authorisation is the
/// identical constant-time digest compare.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SeatToken {
    hash: [u8; 32],
}

impl SeatToken {
    /// Mint a new token: 256 bits of OS entropy, hex-encoded for the wire. Returns
    /// the hashed handle to store and the one-time plaintext to hand to the client.
    pub(crate) fn mint() -> (SeatToken, String) {
        let mut raw = [0u8; 32];
        rand::rngs::SysRng
            .try_fill_bytes(&mut raw)
            .expect("OS CSPRNG unavailable");
        let plaintext = hex::encode(raw);
        (SeatToken::from_plaintext(&plaintext), plaintext)
    }

    /// The hashed handle for a known plaintext — the digest to persist at mint
    /// time, and the verifier used by [`mint`](Self::mint).
    pub(crate) fn from_plaintext(plaintext: &str) -> SeatToken {
        let hash = Sha256::digest(plaintext.as_bytes()).into();
        SeatToken { hash }
    }

    /// Rebuild the handle from a stored SHA-256 digest — the recovery path,
    /// where the `match_registry` persisted the hash, not the plaintext.
    /// The reconnect then authorises a presented token against this exactly as a
    /// live match would (the same [`matches`](Self::matches) digest compare); the
    /// server never reconstructs the clear-text credential.
    pub(crate) fn from_hash(hash: [u8; 32]) -> SeatToken {
        SeatToken { hash }
    }

    /// The stored digest, to persist in the `match_registry` — the
    /// registry only ever receives this hash, never the plaintext, so the
    /// clear-text credential exists solely in the one-time creation response.
    pub(crate) fn digest(&self) -> [u8; 32] {
        self.hash
    }

    /// Does this presented plaintext authorise against the stored hash?
    pub(crate) fn matches(&self, presented: &str) -> bool {
        let h = Sha256::digest(presented.as_bytes());
        ct_eq(&self.hash, h.as_slice())
    }
}

impl std::fmt::Debug for SeatToken {
    /// Never print the digest — a hash is still match-identifying. Tests that
    /// need to assert equality compare `SeatToken`s, not their bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SeatToken(<redacted>)")
    }
}

/// A commit–reveal over a match seed: `SHA-256(seed_le ‖ salt)`.
///
/// Created with [`SeedCommitment::new`] at match start; the server publishes
/// [`SeedCommitment::commit_hex`] in the creation response and keeps the struct.
/// At match end it discloses [`SeedCommitment::reveal`] `{seed, salt}`; any
/// observer recomputes the commitment and checks it equals what was published
/// before play began ([`SeedCommitment::verify`] is that check) — proof the seed
/// was fixed in advance and the shuffle is provably fair.
#[derive(Clone)]
pub(crate) struct SeedCommitment {
    seed: u64,
    salt: [u8; 16],
    commit: [u8; 32],
}

impl SeedCommitment {
    /// Commit to `seed` under a fresh random salt.
    pub(crate) fn new(seed: u64) -> SeedCommitment {
        let mut salt = [0u8; 16];
        rand::rngs::SysRng
            .try_fill_bytes(&mut salt)
            .expect("OS CSPRNG unavailable");
        Self::from_parts(seed, salt)
    }

    /// Re-derive a commitment from a persisted `{seed, salt}` (the recovery
    /// path): a match rebuilt after a restart re-commits under the SAME
    /// salt the registry stored, so its published `commit_hex` is bit-identical to
    /// the one announced at creation — the commit–reveal stays honourable across a
    /// crash, not just within one process. The commitment is recomputed (never
    /// persisted), so it cannot drift from the `{seed, salt}` it derives from.
    pub(crate) fn from_parts(seed: u64, salt: [u8; 16]) -> SeedCommitment {
        let commit = Self::digest(seed, &salt);
        SeedCommitment { seed, salt, commit }
    }

    /// The commitment salt, to persist alongside the match so recovery can
    /// [`from_parts`](Self::from_parts) the identical commitment. Secret until the
    /// end-of-match reveal — it is stored only in the server-side `match_registry`,
    /// never in a view or event.
    pub(crate) fn salt(&self) -> [u8; 16] {
        self.salt
    }

    fn digest(seed: u64, salt: &[u8; 16]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(seed.to_le_bytes());
        h.update(salt);
        h.finalize().into()
    }

    /// The published commitment (hex) — safe to reveal at match start; it hides
    /// the seed (the salt is secret until reveal).
    pub(crate) fn commit_hex(&self) -> String {
        hex::encode(self.commit)
    }

    /// The reveal payload, disclosed when the telling ends: the seed and the salt,
    /// each hex, so an observer can recompute and verify the published commitment.
    pub(crate) fn reveal(&self) -> SeedReveal {
        SeedReveal {
            seed: self.seed,
            salt_hex: hex::encode(self.salt),
            commit_hex: self.commit_hex(),
        }
    }

    /// Independently verify a `(seed, salt) → commit` triple — what a client (or a
    /// test) runs against the published commitment after the reveal. The server
    /// itself never needs to verify its own commitment (it produced it), so this is
    /// the verifier half of the protocol: exercised by the over-the-wire test and
    /// mirrored by out-of-process clients.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn verify(commit_hex: &str, seed: u64, salt_hex: &str) -> bool {
        let Ok(salt_vec) = hex::decode(salt_hex) else {
            return false;
        };
        let Ok(salt) = <[u8; 16]>::try_from(salt_vec.as_slice()) else {
            return false;
        };
        let Ok(commit) = hex::decode(commit_hex) else {
            return false;
        };
        ct_eq(&Self::digest(seed, &salt), &commit)
    }
}

/// The seed reveal handed to clients at match end (a serializable view of the
/// secret half of a [`SeedCommitment`]).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SeedReveal {
    pub seed: u64,
    pub salt_hex: String,
    pub commit_hex: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_seeds_are_not_constant() {
        // Vanishingly unlikely to collide if the CSPRNG is live; a smoke test that
        // we are not handing back a constant (the old SystemTime path was at least
        // monotonic, a constant would be a wiring bug).
        let seeds: std::collections::HashSet<u64> = (0..64).map(|_| fresh_seed()).collect();
        assert!(seeds.len() > 60, "OS CSPRNG seeds should be distinct");
    }

    #[test]
    fn a_minted_token_authorises_only_its_own_plaintext() {
        let (tok, plaintext) = SeatToken::mint();
        assert!(tok.matches(&plaintext), "the issued plaintext authorises");
        assert!(!tok.matches("not-the-token"), "a wrong token is rejected");
        assert!(
            !tok.matches(&plaintext[..plaintext.len() - 1]),
            "a truncated token is rejected"
        );
        // The hash round-trips: recovery hashes the persisted plaintext to the
        // same handle, so a reconnect after a restart still authorises.
        assert_eq!(
            tok,
            SeatToken::from_plaintext(&plaintext),
            "hashing the same plaintext yields the same handle"
        );
    }

    #[test]
    fn a_token_rebuilt_from_its_stored_hash_authorises_the_same_plaintext() {
        // The registry persists the HASH, not the plaintext. Recovery
        // rebuilds the handle from that digest and must authorise exactly as the
        // live match did — the right plaintext in, a stranger's out — without ever
        // reconstructing the clear-text credential.
        let (live, plaintext) = SeatToken::mint();
        let recovered = SeatToken::from_hash(live.hash);
        assert_eq!(recovered, live, "the rebuilt handle equals the live one");
        assert!(
            recovered.matches(&plaintext),
            "the persisted-hash handle authorises its own token"
        );
        assert!(
            !recovered.matches("deadbeef-not-the-token"),
            "a stranger's token fails against the stored hash"
        );
    }

    #[test]
    fn minted_tokens_are_unique_and_high_entropy() {
        let (_, a) = SeatToken::mint();
        let (_, b) = SeatToken::mint();
        assert_ne!(a, b, "two mints differ");
        assert_eq!(a.len(), 64, "256 bits, hex-encoded");
    }

    #[test]
    fn debug_never_prints_the_hash() {
        let (tok, _) = SeatToken::mint();
        assert_eq!(format!("{tok:?}"), "SeatToken(<redacted>)");
    }

    #[test]
    fn a_commitment_verifies_against_its_reveal() {
        let seed = 0xDEAD_BEEF_0000_1234u64;
        let c = SeedCommitment::new(seed);
        let r = c.reveal();
        assert_eq!(r.seed, seed);
        assert!(
            SeedCommitment::verify(&r.commit_hex, r.seed, &r.salt_hex),
            "the honest reveal verifies against the published commitment"
        );
        // A tampered seed (the server trying to rewrite history after commit) fails.
        assert!(
            !SeedCommitment::verify(&r.commit_hex, seed ^ 1, &r.salt_hex),
            "a different seed cannot match the commitment"
        );
        // A tampered salt fails too.
        assert!(
            !SeedCommitment::verify(&r.commit_hex, seed, "00000000000000000000000000000000"),
            "a different salt cannot match the commitment"
        );
    }

    #[test]
    fn distinct_seeds_commit_differently_and_a_seed_hides() {
        let a = SeedCommitment::new(1);
        let b = SeedCommitment::new(2);
        assert_ne!(
            a.commit_hex(),
            b.commit_hex(),
            "different seeds → different commits"
        );
        // Even the SAME seed commits differently under a fresh salt — the commitment
        // is hiding, so an observer can't grind the small seed space before reveal.
        let c1 = SeedCommitment::new(42);
        let c2 = SeedCommitment::new(42);
        assert_ne!(
            c1.commit_hex(),
            c2.commit_hex(),
            "the random salt hides the seed pre-reveal"
        );
    }

    #[test]
    fn a_commitment_recovered_from_persisted_seed_and_salt_is_identical() {
        // A match recovered after a restart re-commits from the PERSISTED
        // {seed, salt} and must reproduce the
        // commitment published at creation — bit-for-bit. (Before persisting the
        // salt, recovery re-committed under a FRESH salt and the published
        // commitment could no longer be honoured.)
        let seed = 0xFEED_FACE_DEAD_BEEFu64;
        let original = SeedCommitment::new(seed);
        let recovered = SeedCommitment::from_parts(seed, original.salt());
        assert_eq!(
            recovered.commit_hex(),
            original.commit_hex(),
            "the recovered commitment honours the one published before play began"
        );
        // And the recovered reveal still verifies against that original commitment.
        let r = recovered.reveal();
        assert_eq!(r.seed, seed);
        assert!(
            SeedCommitment::verify(&original.commit_hex(), r.seed, &r.salt_hex),
            "the recovered reveal verifies against the originally-published commitment"
        );
    }

    #[test]
    fn verify_rejects_malformed_hex() {
        let c = SeedCommitment::new(7);
        let r = c.reveal();
        assert!(!SeedCommitment::verify("nothex", r.seed, &r.salt_hex));
        assert!(!SeedCommitment::verify(&r.commit_hex, r.seed, "zz"));
    }
}
