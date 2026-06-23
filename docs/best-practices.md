# Best Practices — a Rust repository worked by humans and agents

This is the standard this repository holds itself to. It merges industry
practice with what we've proven here. Every practice is phrased so a machine
or a human can check it; where a gate exists, the gate is named. Audits
against this document live in `docs/decisions/`.

## 1. Principles

**P1 — Documents are law; code follows.** Design documents state intent;
code implements it; tests cite it. A change of behavior starts as a doc
change. When code and doc disagree, the code is wrong until the doc is
amended.

**P2 — One source of truth per domain, and it is never the generated copy.**
Card data lives in `app/crates/recollect-core/data/cards.toml`; `catalog.json` and
the runtime side-data (`effects.json`, `evolution_{lines,split}.json`,
`card_{keys,keywords}.json`) are generated from it (`make catalog`) and CI diffs the
catalog (`make catalog-check`). Hand-editing a generated artifact is a defect even
when the edit is correct.

**P3 — Machine-checkable beats prose.** An invariant worth writing down is
worth a test: drift gates for generated artifacts, ratchet tests for
implementation coverage, property tests for laws. Prose explains *why*; only
tests can promise *that*.

**P4 — Honest state, named levers.** Debt is documented where it lives, with
its resolution event attached (the condition under which it gets paid down, and
the test that will gate it). A risk taxonomy — FIXED / ACCEPTED / OPEN /
VERIFIED — applies to all risk statements: nothing is silently deferred.

**P5 — One done-gate, same for everyone.** `make test && make catalog-check`
defines done for a human at a keyboard and an agent in a loop. If "done"
requires tribal knowledge, the repository is broken.

**P6 — Every interaction is animated to a signature-tier bar.** Every
interaction, transition, cue, and state-change is **animated** to a solid,
polished, signature-tier standard — subtle, obvious behavior the player picks up
instantly. Nothing snaps or pops without intent; a piece selecting, a card
lifting, a phase turning, a banish, the Dusk falling, the verdict landing each get
a crafted motion the player reads without being told. This is the interaction
expression of the signature-tier bar (AGENTS.md): not decoration bolted on at the
end, but how state-change *is communicated*. Reduced motion is honored
(`prefers-reduced-motion`); the cue survives, the flourish steps aside.

## 2. Layout

- A workspace of focused crates under `app/crates/`, each with a crate-level
  `//!` stating its purpose and boundaries.
- `docs/` holds the law (design docs), the guides (`testing.md`,
  `operations.md`, this file), and `docs/decisions/` — the design rationale
  that explains *why the code is shaped this way* (the web/UX design-of-record,
  the brand + a11y bars, the bot/ML and launch plans). Agents must be able to
  reach every decision from the repo.
- `AGENTS.md` at root is the canonical contributor guide, tool-agnostic.
  Tool-specific files (`CLAUDE.md`) are one-line pointers to it — one
  source of truth, no drift.
- `Makefile` is the universal verb interface: build, test, run, deploy,
  teardown. New capabilities ship with a make target and a `## help` line.

## 3. Toolchain and dependencies

- **One toolchain**, pinned in `rust-toolchain.toml`; CI, Docker, and dev
  machines match it exactly. No MSRV range to maintain — one version, moved
  deliberately.
- Manifests carry **semver ranges** (policy); `Cargo.lock` is committed
  (reproducibility). A **scheduled freshness job** runs `cargo update` +
  full tests weekly so the lock never fossilizes.
- **Supply chain is gated**: `cargo deny check` (licenses, advisories,
  duplicate majors) runs in CI. A dependency is a liability with a
  changelog; prefer the standard library and small, boring crates.
- **`#![forbid(unsafe_code)]`** in every crate that doesn't need it; a
  crate that needs it documents each block with a safety argument.

## 4. Code quality

- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are
  blocking CI gates. Style debates are settled by the formatter, once.
- `RUSTDOCFLAGS="-D warnings" cargo doc` gates documentation: every public
  item documented, links valid. Crate roots carry `//!` overviews that
  state purpose, boundaries, and the invariants the crate defends.
- Errors are typed (`thiserror`) at library boundaries. `unwrap()` is
  acceptable only where the invariant is local and stated; serving paths
  return errors.
- Comments explain *why* and cite the design doc section or the decision
  record that motivated the shape.

## 5. Testing

- **A written taxonomy** (`docs/testing.md`): unit (one law per test, named
  for the law), property (invariants under random legal play), fuzz/security
  (hostile input rejected without state change), determinism (seed + commands
  ⇒ identical state), redaction (views leak nothing), integration (against
  real services in CI), protocol round-trips.
- **Red tests are executable specifications**: unbuilt features live as
  `#[ignore = "reason citing the doc"]` tests. The ignore reason is the
  contract. They are implemented, never deleted.
- **Ratchet tests** make coverage honest: counts of implemented-vs-pending
  may only move toward implementation, enforced by assertion.
- **A bug fix ships with the test that would have caught it.** No flaky
  tests: anything nondeterministic takes a seed.
- Monitoring (probe fleets, tripwires) is distinct from testing and runs on
  a schedule, feeding evidence back into the design docs.

## 6. Working with agents

- Agents read `AGENTS.md` first; it states the invariants, the done-gate,
  the layout, and the workflow order (doc → code → test).
- **Context is in the repo**: the design rationale lives in files, not chat
  history. An agent with a fresh context must be able to reconstruct *why* from
  `docs/decisions/`.
- Generated artifacts declare their generator in a header comment; the make
  target regenerates, CI diffs.
- Agents and humans use the same gates. There is no "agent mode" with
  looser checks.

## 7. CI/CD

- Every gate above is a blocking CI job: tests, drift, the determinism
  invariant, fmt, clippy (`-D warnings`), deny, rustdoc,
  integration-with-services, cross-target builds (wasm), helm lint, and the
  container image build. (OpenTelemetry is always compiled in, so the normal
  build/test covers it — no separate feature-build job.) Subjective clippy
  lints are centrally allowed in `[workspace.lints]`; correctness lints gate.
- Images are pinned-base, multi-stage, distroless, nonroot; tags are
  immutable (the chart *requires* a tag — `:latest` cannot deploy).
- A green main is the only deployable artifact source.

## 8. Security and operations

- **Secrets never enter git** — charts take `existingSecret` references;
  compose credentials are local-only by name. Tokens are stored hashed,
  shown once.
- All SQL is parameterized; capability strings (seat/account tokens) stay out
  of URLs — a seat token arrives in the first WebSocket frame, never the path.
- **Teardown has rungs**: backup target → volume-preserving down →
  confirmation-gated destruction.
- **Red-team passes are scheduled work**, not incident response: every
  platform milestone gets one, findings get numbers, and numbers get
  resolved or accepted in writing.

## 9. Git and review

- Conventional, present-tense commit subjects; the body cites doc sections
  and finding numbers. Small changes that each keep the done-gate green.
- PR templates ask the three questions: which doc changed first, which
  tests own this, did the gates run.
