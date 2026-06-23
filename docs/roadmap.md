# Roadmap — the forward backlog

This is the single source of truth for **what to build next**. It is a clean,
forward-looking backlog — not a changelog. What the engine and product already do
lives in `architecture.md` (what exists) and the design/cards docs (the law);
this file lists only **open work**.

The list is the claim queue for parallel work: take the top unclaimed item that
fits — each carries enough scope to start cold. Tag an item ⏳ with your session
when you claim it; remove it once it lands and its substance has moved into the
living docs. Order is roughly **website/play-stack first, native shells last**, but
within a tier pick by fit.

The product is **launch-ready**: the full play stack (1v1-vs-bot, 2v2, and
human-vs-human PvP), the canvas-native client with its a11y tree, anonymous
identity, the journal, the deploy IaC, and the observability stack are all built.
The remaining launch step is the operator's `pulumi up` (foundation once → CI
image push → platform) followed by a browser-verify of the live box; both need a
human at a real machine. The items below are everything that is still genuinely
open.

## Launch tail (a human at a real machine)

- **Go-live operation.** Run `pulumi up` for the launch host — the `foundation`
  project once (ECR repo + GitHub-OIDC CI push role), then `platform` per release
  (EC2 + Cloudflare Tunnel; the box pulls the CI-pushed ECR image) — point DNS
  through the Tunnel, and walk `docs/manual_verification.md`: browser boot, real
  mouse/touch/keyboard, multi-client play, and the LGTM observability stack behind
  Cloudflare Access. Inputs and the SSO/Tunnel runbook are in `deploy/README.md`.
- **Accessibility — real-AT verification.** The canvas a11y tree is now a complete
  per-tile ARIA grid (`role=grid`/`row`/`gridcell` with row/col indices, axe WCAG 2.1 AA
  clean — it announces "row R, column C, occupant" and arrow-navigates). The residual is a
  manual pass with a **real screen reader** (VoiceOver / NVDA) confirming the grid-navigation
  announcements aren't chatty — a `manual_verification.md` checklist item, a human-at-a-machine
  check, not a code task. Bar: `docs/decisions/brand_and_accessibility.md`.

## Balance

- **2v2-Solace too easy (~75%).** The open 1v1-vs-Solace residual is a **structural**
  gap, not a knob: a depth-2 Solace pair sharing one eval can't wall the 6×6, and no
  `(temp,depth)` pulls it into band. Closing it is **coordination/eval work**
  (policy-net-adjacent), tracked under the policy-net milestone, not a tuning pass.
  Measure per disposition with `char_sweep`/`char_sweep_2v2`; the fairness gate is
  `solace_winnability`.
- **Open balance dials (evidence-gated).** The bot **action-mix probe** is done
  (`docs/decisions/bot_action_mix.md` — per-decision %, all tiers/factions): the mix is
  human-sensible, with evolve/devolve firing near-zero **correctly** for curve-tuned decks
  and Glimpse healthy (rises with skill, front-loaded), so **no eval change is indicated**.
  Two dials remain, folded into other work: evolution **deck-density** (the real lever is the
  post-launch deck-builder's evolution-aware density, not a tier knob) and the **Glimpse**
  payoff (re-check only if a future eval change moves the mix). `difficulty.md` carries the
  per-disposition numbers.

## Player-facing build (post-launch)

- **Deck-builder.** A player-facing builder over the catalog, enforcing the shared
  deck standards (`validate_deck_for`: size 20, singleton ≤ 1, faction purity) —
  those and `generate_deck_for` already exist. It must enforce the
  **no-orphan-evolution** rule (a form in the deck requires at least one of its base
  forms) and surface the base↔form pairing so a player never assembles a form they
  can never land. It also owns the player's **evolution-aware deck density** — a
  hand-built deck's evolution play should be a choice, not luck.
- **Rich combat Forecast (the visual panel).** The core forecast is wired — every legal
  move carries an exact local `ForecastView` (damage both ways, banishes, Echo-live),
  surfaced in the **action labels** (`Play Cinderling → c3 ⚔ d3`). Open is the **rich
  visual layer** the design describes: an on-canvas forecast overlay (the outcome drawn on
  the board tiles, `scene.rs`/`render.rs`) + the one-tap Forecast panel showing
  **interception** (a covering enemy striking your arrival), **Promise** redirections, and
  the **"−10 (unseen)"** hidden-aura hints. Post-launch UX-depth — the information is already
  readable from the move text; this is the visual polish.
- **Card & character images.** The delivery pipeline is built (placeholders ship,
  WebP delivery, a `make cards-check` gate, the `tools/cardpipe` tool kept outside the
  workspace). Open is the **real art**: (1) generate masters via an image service
  keyed by each card's stable `key`, drop them at `assets/cards-src/<key>.png`, run
  `make cards-images`; (2) wgpu in-client card textures (an atlas keyed by `key`) in
  `recollect-web` `scene.rs`/`render.rs`, then re-measure the wasm budget; (3) the
  **character + bot-player portraits** (the 20 Lorekeeper + 20 Solace characters'
  avatars — an initial today). As-built: `docs/decisions/card_images.md`.
- **Music & sound.** The audio layer for the "Living Ink" sound grammar (design §16:
  paper-and-water sound; the Solace as the absence of room tone — the Unwritten make
  no sound). The nav already carries a sound toggle; this builds the cues + score.
  Honors the a11y rule — audio always has a visual cue, never required to play.
- **GPU web-gallery video clips.** The canvas set-piece clips (ink-bloom write-on,
  unwrite/dissolve, the Dusk/Nightfall contraction) need a real GPU to capture —
  record on a Mac with a GPU and drop them into the gallery. The stills are committed;
  only the moving captures wait on hardware.
- **Fixed signature Lorekeeper characters (optional).** The 20 Lorekeeper characters
  are generated on-theme today (a lore name + an archetype lean + a seed-salted
  deck-bias over the 234-card pool, the twin of the Solace character model). The open
  question is whether to promote any to **fixed, signature** decks rather than
  generated — a content/identity call, not a correctness one.

## Accounts & identity (post-launch)

- **Full accounts + httpOnly auth cookies.** The durable account layer over the
  anonymous `localStorage` handle that ships at launch: OIDC sign-in
  (Google/Apple/email per `docs/decisions/playtest_launch_plan.md` §1), sessions in
  **httpOnly** cookies (not JS-readable tokens), an anon handle **claimable** into an
  account with history carried over, the account↔match-token boundary preserved
  (match tokens stay short-lived per-seat). Gated behind the COPPA/GDPR workstream;
  anonymous play ships first.

## Engineering quality (ongoing)

- **Survivor kill-pass — high-value slice DONE, geometry deferred.** The pre-launch full-core
  sweep (cargo-mutants, **3367 mutants in 3 h**) ran clean: **0 correctness-critical survivors**
  (combat 0 — it re-confirmed #104's combat tests + #108's Overwrite-bond fix), 275 survivors
  all test-*precision* gaps. The high-value medium slice is now **killed**: the **decide-handler
  reject-paths** (42 outcome-asserting tests, 9e34082), **validate_deck** boundaries (8 tests),
  and the **RNG/EntropySource math** (adopted ironstate's `assert_entropy_contract` + the
  golden pins, 6b7bd99). The **remaining ~110** — `legal_commands` + reach geometry
  (`projection`/`types`) + effect-clause predicates (`support`) — are edge offsets reach/combat
  already exercise; deferred to the periodic sweep. The **~135 low** (deck-gen heuristics,
  invariant-check boundaries, accessors/metadata) stay justify/won't-fix.
- **Mutation testing — the periodic sweep.** `make mutants` (cargo-mutants; `--jobs`-parallel,
  ~3 h full-core on 18 cores) + its `workflow_dispatch` job are wired, with an
  `app/.cargo/mutants.toml` equivalent-mutant skip-list. Stays a long-running periodic job,
  never the fast suite; re-run after the kill-pass to confirm the medium survivors are gone.
- **Forfeit/abandonment transport wiring + its metric — DONE.** The engine already resolved
  `Command::MatchAbandoned` to a clean `Win(present_seat)` forfeit (journaled distinctly as
  `Event::MatchAbandoned`, excluded from `legal_commands`); the **server now issues it** on a
  grace-expired disconnect. The transport seam: the per-seat `socket_loop` signals the match
  actor (`MatchCmd::SeatVacated`) the instant a socket tears down; the actor arms a per-principal
  grace timer (`tokio::select!` over the command channel + the earliest deadline), a reconnect
  disarms it, and on expiry it issues `MatchAbandoned` against the absent seat's TEAM through the
  same journaled-or-in-memory apply funnel the command path uses (so it journals + replays via
  `resume_async`). **Only human seats arm** — a bot has no socket and never forfeits; in **2v2**
  any one absent slot forfeits its whole team (the seat-shaped command IS the team). Grace is
  `?abandon_grace_secs=N` (default **120s**; `0` disables it, which the reconnect tests use).
  Instrumented as a `reason=played_out|abandoned` label on `recollect.matches.finished` plus a
  bare `recollect.matches.forfeits` counter (see `docs/observability.md`).

## Later milestones

- **Policy-net agent (post-launch ceiling).** The classical bot is strong enough to
  launch on (effect-aware, a calibrated monotonic difficulty ladder). The learned net
  is a post-launch ceiling, not a now-item: self-play data → a small distilled net →
  an eval ladder versus the greedy bot, swapped in behind the same `choose` seam.
  Standing decision: grace-class design levers may only be revisited with policy-net
  evidence. Plan: `docs/decisions/bot_and_ml_plan.md`.
- **Native iOS / Android shells (dead last).** The Swift + Android apps over
  `recollect-ffi` (the UniFFI surface over core is in; `make ffi-bindings` generates
  valid Swift + Kotlin from the built cdylib). Open: the actual Xcode/Gradle projects
  rendering the shared wgpu view, packaging (xcframework/AAR), a CI gate that
  regenerates the bindings, and App Attest / Play Integrity. The renderer and
  persistence are already shared with the web client.
- **The K8s scale target.** The free-tier EC2 host launches the game; Kubernetes
  (the §10.2 EKS plan + `deploy/helm/`) is for real traffic, not the playtest. Stand
  it up when load demands it.

## Open design levers (held, evidence-gated)

These are deliberately unbuilt — recorded so the rationale isn't lost, each gated on
evidence (mostly the policy-net milestone or live data). The design doc §17 carries
the full lever list; the live ones:

- **Going-second compensation** (the Listener's tie-break / round-aware grace) —
  removed as a rule; the lever stays holstered, to be added only if first-player
  win-rate data shows imbalance.
- **Deck size 20 → 22** — the dial for the late-game draw tail if it thins.
- **Befriend-priority window** — the Stray-sniping counterplay lever.
- **2v2 round count and second-team grace** — open tuning items for the team format.

The Souvenir (a shareable, provably-fair match token) is a v2/post-launch surface;
the correspondence/async mode is out of scope for v1 (the engine's system-kind
abandonment command is the seam if it ever ships).
