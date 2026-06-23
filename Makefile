# RECOLLECT — ops entry points. `make help` lists everything.
# The Rust workspace lives in app/; this Makefile orchestrates from the repo
# root (where docs/, tools/, deploy/, and the Docker files live).
SHELL := /bin/bash
APP := app
COMPOSE := docker compose -f docker-compose.yml
PG_URL := postgres://recollect:recollect-local-only@localhost:5432/recollect

help: ## list targets
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) | awk 'BEGIN{FS=":.*?## "};{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

test: ## fast suite (excludes the slow model-checker + live-server integration; see test-verify / test-slow)
	command -v cargo-nextest >/dev/null || cargo install --locked cargo-nextest
	cd $(APP) && cargo nextest run --workspace --exclude recollect-verify
	cd $(APP) && cargo test --workspace --exclude recollect-verify --doc

test-verify: ## the recollect-verify model-checker (slow, ~10s; run separately)
	cd $(APP) && cargo test -p recollect-verify --test solace_bridge

test-slow: ## slow real-server integration (cli↔server over a live socket; run separately / nightly)
	cd $(APP) && cargo test -p recollect-cli --test online_roundtrip -- --ignored

test-all: test test-verify test-slow ## fast suite + model-checker + live-server integration

# --- long-running / nightly verification (manually runnable now; CI nightly schedules them) ---
# The canonical gameplay fuzz is the FULL-CATALOG playthrough red-team
# (tests/suites/fuzz.rs): the 3 mode playthroughs + the determinism-replay
# and rejection arms, all over the canon 419-card set. It's a consolidated module, so we select
# it by TEST-NAME filter against the crate's `main` integration binary (in release).
# The libtest harness takes MULTIPLE name filters, but only after `--` (cargo's own positional
# TESTNAME is single); so the arm filters ride alongside `--nocapture` there.
FUZZ_ARMS := playthroughs_hold_every_invariant canon_replays_are_bit_identical canon_rejected_commands_leave_no_trace
fuzz: ## full-catalog gameplay fuzz: make fuzz SEEDS=20000 [BASE=0] | make fuzz SECONDS=60 (wall-clock bounded)
	cd $(APP) && RT_SEEDS=$(SEEDS) RT_SEED_BASE=$(or $(BASE),0) FUZZ_SECONDS=$(SECONDS) cargo test -p recollect-core --release -- $(FUZZ_ARMS) --nocapture

soak: ## long full-catalog fuzz soak for a fixed interval: make soak SECONDS=1800 (default 300)
	cd $(APP) && FUZZ_SECONDS=$(or $(SECONDS),300) cargo test -p recollect-core --release -- $(FUZZ_ARMS) --nocapture

mutants: ## mutation testing — do the tests catch bugs? make mutants FILE='**/engine/*.rs' [TIMEOUT=30]
	cd $(APP) && cargo mutants -p recollect-core --timeout $(or $(TIMEOUT),30) $(if $(FILE),-f $(FILE),)

audit: ## dependency vulnerability scan — RUSTSEC advisories over Cargo.lock (installs cargo-audit on demand)
	command -v cargo-audit >/dev/null || cargo install cargo-audit --locked
	cd $(APP) && cargo audit

nightly: ## the whole nightly verification suite locally (soak + model-check + golden + mutants + audit)
	$(MAKE) soak SECONDS=$(or $(SECONDS),600)
	$(MAKE) test-verify
	cd $(APP) && cargo test -p recollect-core --test golden_replay
	$(MAKE) mutants FILE=$(or $(FILE),'**/effects.rs')
	$(MAKE) audit

sim: ## headless balance simulation: make sim N=10000
	cd $(APP) && cargo run --release -p recollect-bot -- $(or $(N),10000)

probes: ## nightly balance/monitoring probes under the law
	cd $(APP) && cargo run --release -p recollect-bot --bin probes

server: ## run the server locally (in-memory unless DATABASE_URL is set)
	cd $(APP) && cargo run -p recollect-server

client: ## network play: create a match and join seat A (prints seat B token)
	cd $(APP) && cargo run -p recollect-cli -- online new

client-join: ## join an existing match: make client-join ID=<match> TOKEN=<seat-token>
	cd $(APP) && cargo run -p recollect-cli -- online join $(ID) $(TOKEN)

fmt: ## format the workspace (CI enforces --check)
	cd $(APP) && cargo fmt

lint: ## clippy with warnings-as-errors, plus supply-chain check if installed
	cd $(APP) && cargo clippy --all-targets -- -D warnings
	@command -v cargo-deny >/dev/null && (cd $(APP) && cargo deny check) || echo "(cargo-deny not installed locally; CI enforces it)"

doc: ## build rustdoc for the workspace (output in app/target/doc)
	cd $(APP) && cargo doc --workspace --no-deps

tui: ## play in the terminal vs the bot (local, offline)
	cd $(APP) && cargo run -p recollect-cli

tui-gallery: ## regenerate the committed TUI text stills (docs/gallery/tui/) from a seeded engine
	tools/gen_tui_gallery.sh

tui-shots: ## regenerate the committed TUI image gallery — real PNG screenshots + a GIF (needs vhs+ttyd+ffmpeg; skips cleanly if absent)
	tools/gen_tui_shots.sh

web-gallery: ## regenerate the committed web canvas stills (docs/gallery/web/) from the GPU-free CPU rasterizer
	tools/gen_gallery.sh

catalog: ## regenerate the canon catalog + side-data from the TOML source, then test
	python3 tools/gen_catalog.py
	cd $(APP) && cargo test -p recollect-core --test canon

catalog-check: cards-validate ## CI gate: catalog must match the TOML source exactly
	python3 tools/gen_catalog.py $(APP)/crates/recollect-core/data/cards.toml /tmp/catalog.check.json
	diff -q /tmp/catalog.check.json $(APP)/crates/recollect-core/data/catalog.json

cards-validate: ## lint the card source: every [[card]] has its required fields, well-formed
	python3 tools/validate_cards.py $(APP)/crates/recollect-core/data/cards.toml

site: ## build the deployable static site into dist/ (catalog + lore pages generated; wasm play client via trunk if installed)
	python3 tools/gen_cards_page.py
	python3 tools/gen_lore_page.py
	rm -rf dist && mkdir -p dist && cp -R site/. dist/
	@command -v trunk >/dev/null \
		&& (cd $(APP)/crates/recollect-web && trunk build --release --public-url /client/) \
		&& mkdir -p dist/client && cp -R $(APP)/crates/recollect-web/dist/. dist/client/ \
		|| echo "(trunk not installed — static site assembled; wasm play client skipped. Install: cargo install --locked trunk)"
	@echo "site -> dist/  (open dist/index.html, or run: make site-serve)"

site-serve: site ## build the site, then serve it locally at http://localhost:8000
	cd dist && python3 -m http.server 8000

# Quarantined Node tooling (tools/uitest/) — Playwright deps NEVER touch the cargo
# graph. The play client's wgpu canvas needs a real GL surface, so Playwright runs
# HEADED full Chromium; on a Linux runner that means wrapping in `xvfb-run` (a
# virtual display). On a desktop (macOS/Windows) it just runs headed. See
# docs/testing.md ("UI / end-to-end") + docs/operations.md.
UITEST_DIR := tools/uitest
XVFB := $(if $(shell command -v xvfb-run 2>/dev/null),xvfb-run -a,)
uitest: site ## build the site, then drive it in a headless browser with Playwright (UI/e2e)
	cd $(UITEST_DIR) && npm ci || (cd $(UITEST_DIR) && npm install)
	cd $(UITEST_DIR) && npx playwright install chromium
	cd $(UITEST_DIR) && $(XVFB) npx playwright test

uitest-update: site ## refresh the committed visual-regression baselines (run on the same OS that asserts them)
	cd $(UITEST_DIR) && npm ci || (cd $(UITEST_DIR) && npm install)
	cd $(UITEST_DIR) && npx playwright install chromium
	cd $(UITEST_DIR) && $(XVFB) npx playwright test --update-snapshots

# --- wgpu PIXEL/VISUAL goldens — a SEPARATE, DECOUPLED target (not part of `make uitest`) ---
# Diffs the REAL wgpu canvas render against committed golden PNGs (visual-canvas.spec.ts).
# This is the FLAKIEST lane (GPU/driver/anti-aliasing variance), so it is gated behind
# UITEST_VISUAL=1 and run on its own — ignorable if flaky, droppable if it can't be tuned
# (drop-if-flaky policy in docs/testing.md). Needs a real GL surface (a display, or xvfb on
# Linux); on a no-GPU sandbox the specs SKIP cleanly (the canvas never mounts). The golden
# PNGs are BINARY — the maintainer merges this binary-bearing lane via cherry-pick.
uitest-visual: site ## wgpu pixel/visual goldens ONLY (separate from uitest; needs a real GPU — skips cleanly without one)
	cd $(UITEST_DIR) && npm ci || (cd $(UITEST_DIR) && npm install)
	cd $(UITEST_DIR) && npx playwright install chromium
	cd $(UITEST_DIR) && UITEST_VISUAL=1 $(XVFB) npx playwright test visual-canvas.spec.ts

uitest-visual-update: site ## refresh the committed wgpu canvas goldens (run on a real GPU, same OS that asserts them)
	cd $(UITEST_DIR) && npm ci || (cd $(UITEST_DIR) && npm install)
	cd $(UITEST_DIR) && npx playwright install chromium
	cd $(UITEST_DIR) && UITEST_VISUAL=1 $(XVFB) npx playwright test visual-canvas.spec.ts --update-snapshots

determinism-check: ## invariant: the engine draws only from its own counter-mode Rng (no rand)
	@cd $(APP) && if grep -rnE 'use +rand|rand::|rand_chacha|rand_core' crates/recollect-core/src/; then \
		echo "DETERMINISM VIOLATION: recollect-core source draws from rand; entropy must stay counter-mode"; exit 1; \
	else echo "determinism OK: recollect-core draws only from its own counter-mode Rng"; fi

ffi-bindings: ## D-25: generate the Swift + Kotlin native bindings from the FFI cdylib (output in app/target/bindings/)
	cd $(APP) && cargo build -p recollect-ffi
	cd $(APP) && LIB=$$(ls target/debug/librecollect_ffi.dylib target/debug/librecollect_ffi.so 2>/dev/null | head -1) && \
		for lang in swift kotlin; do \
			cargo run -q -p recollect-ffi --bin uniffi-bindgen -- generate --library "$$LIB" --language $$lang --out-dir target/bindings --no-format; \
		done && echo "native bindings → app/target/bindings/ (swift + kotlin)"

wasm-diff: ## D-26: the seeded playout hashes identically native vs wasm32 (needs wasmtime + the wasm32-wasip1 target; CI runs this)
	@command -v wasmtime >/dev/null || { echo "(wasmtime not installed — 'curl https://wasmtime.dev/install.sh -sSf | bash'; CI runs this gate)"; exit 0; }
	cd $(APP) && cargo run -q -p recollect-determinism > /tmp/det-native.txt \
		&& cargo build -q -p recollect-determinism --release --target wasm32-wasip1 \
		&& wasmtime target/wasm32-wasip1/release/recollect-determinism.wasm > /tmp/det-wasm.txt \
		&& diff /tmp/det-native.txt /tmp/det-wasm.txt && echo "wasm32 differential OK: native == wasm"

up: ## start postgres + grafana (LGTM) + server (build)
	$(COMPOSE) up -d --build
	@echo "grafana:  http://localhost:3000   server: http://localhost:8080/healthz"

# Local/smoke layer the BUILD overlay (docker-compose.build.yml) so the server image is COMPILED
# locally — production omits it and PULLS the server image from ECR (the FOUNDATION/PLATFORM split;
# see deploy/README.md). The local overlay then publishes the port, scales the tunnel/observability
# to zero, and supplies a throwaway Postgres password.
DEPLOY_COMPOSE := docker compose \
	-f deploy/compose/docker-compose.deploy.yml \
	-f deploy/compose/docker-compose.build.yml \
	-f deploy/compose/docker-compose.local.yml
DEPLOY_LOCAL_ENV := POSTGRES_PASSWORD=recollect-local-only

deploy-local: ## run the FULL deploy stack locally (server serves the site at :8080; no tunnel) — play the real website end-to-end
	$(DEPLOY_LOCAL_ENV) $(DEPLOY_COMPOSE) up -d --build
	@echo "website + game: http://localhost:8080   (server serves the static site + wasm client, on-box Postgres)"

deploy-smoke: ## build the deploy image + SMOKE-TEST the running artifact from outside (site + game + journal); always tears down
	COMPOSE_CMD="$(DEPLOY_COMPOSE)" $(DEPLOY_LOCAL_ENV) bash deploy/smoke.sh

deploy-local-down: ## stop the local deploy stack (KEEPS its volume)
	$(DEPLOY_LOCAL_ENV) $(DEPLOY_COMPOSE) down

deploy-local-logs: ## tail the local deploy stack's server logs
	$(DEPLOY_LOCAL_ENV) $(DEPLOY_COMPOSE) logs -f server

# --- FOUNDATION (Pulumi: run ONCE — ECR repo + GitHub OIDC + the scoped CI push role). See -----
# --- deploy/README.md "FOUNDATION" for the SSO admin login + wiring the outputs into GitHub. ----
FOUNDATION := cd deploy/pulumi/foundation &&
foundation-install: ## install the FOUNDATION Pulumi program's deps (run once)
	$(FOUNDATION) npm install
foundation-typecheck: ## type-check the FOUNDATION program (no cloud calls)
	$(FOUNDATION) npx tsc --noEmit
foundation-preview: ## preview the FOUNDATION plan (ECR + OIDC + CI role; needs admin AWS creds)
	$(FOUNDATION) pulumi preview
foundation-up: ## CREATE/UPDATE FOUNDATION (run once, with short-lived admin SSO creds)
	$(FOUNDATION) pulumi up
foundation-outputs: ## print FOUNDATION outputs (repoUrl, ciRoleArn — wire into GitHub vars + PLATFORM)
	$(FOUNDATION) pulumi stack output
foundation-destroy: ## TEAR DOWN FOUNDATION (rare — retires the ECR repo + CI trust; asks first)
	@read -p "This destroys the ECR repo + OIDC provider + CI role (CI can no longer push). Type 'unwrite' to confirm: " c && [ "$$c" = "unwrite" ]
	$(FOUNDATION) pulumi destroy

# --- PLATFORM (Pulumi: run PER RELEASE — AWS EC2 + Cloudflare Tunnel; the box PULLS the server ---
# --- image from FOUNDATION's ECR). See deploy/README.md "PLATFORM" for the full inputs/secrets --
# --- list and the exact `pulumi config set [--secret]` for each (incl. serverImage). ------------
PULUMI := cd deploy/pulumi/platform &&
deploy-install: ## install the PLATFORM Pulumi program's deps (run once)
	$(PULUMI) npm install
deploy-typecheck: ## type-check the PLATFORM program (no cloud calls)
	$(PULUMI) npx tsc --noEmit
deploy-preview: ## preview the live infra plan (needs AWS + Cloudflare creds + stack config)
	$(PULUMI) pulumi preview
deploy-up: ## CREATE/UPDATE the live infra (EC2 + Cloudflare Tunnel + DNS + budgets); the box PULLS the image
	$(PULUMI) pulumi up
deploy-refresh: ## reconcile Pulumi state with real cloud resources
	$(PULUMI) pulumi refresh
deploy-outputs: ## print stack outputs (site URL, instance id, SSM session command)
	$(PULUMI) pulumi stack output
deploy-ssm: ## open a keyless admin shell on the box via SSM Session Manager
	$$($(PULUMI) pulumi stack output ssmSession)
deploy-destroy: ## TEAR DOWN the PLATFORM infra (EC2 + Cloudflare; asks first). FOUNDATION stays up.
	@read -p "This destroys the EC2 box + Cloudflare tunnel/DNS. Type 'unwrite' to confirm: " c && [ "$$c" = "unwrite" ]
	$(PULUMI) pulumi destroy

seed: ## create two demo accounts and a match (prints tokens ONCE)
	@curl -sf -X POST localhost:8080/accounts -H 'content-type: application/json' -d '{"handle":"demo-ari"}' || true; echo
	@curl -sf -X POST localhost:8080/accounts -H 'content-type: application/json' -d '{"handle":"demo-mara"}' || true; echo
	@curl -sf -X POST localhost:8080/matches; echo

db-test: ## run the postgres integration tests against the compose database
	cd $(APP) && PG_URL=$(PG_URL) cargo test -p recollect-journal-postgres -p recollect-server -- --ignored

db-backup: ## pg_dump the local database to backups/ (run before nuke)
	@mkdir -p backups
	$(COMPOSE) exec -T postgres pg_dump -U recollect recollect > backups/recollect-$$(date +%Y%m%d-%H%M%S).sql
	@ls -t backups | head -1

down: ## stop containers, KEEP volumes (safe teardown)
	$(COMPOSE) down

nuke: ## stop containers AND DELETE volumes (asks first)
	@read -p "This deletes the local database and dashboards. Type 'unwrite' to confirm: " c && [ "$$c" = "unwrite" ]
	$(COMPOSE) down -v

logs: ## tail server logs
	$(COMPOSE) logs -f server

helm-lint: ## lint the chart (requires helm)
	helm lint deploy/helm/recollect

helm-template: ## render manifests locally (requires helm)
	helm template recollect deploy/helm/recollect

.PHONY: help test test-verify test-all fuzz soak mutants audit nightly sim probes server client client-join fmt lint doc tui tui-gallery tui-shots web-gallery catalog catalog-check cards-validate site site-serve uitest uitest-update uitest-visual uitest-visual-update determinism-check up seed db-test db-backup down nuke logs helm-lint helm-template deploy-local deploy-smoke deploy-local-down deploy-local-logs foundation-install foundation-typecheck foundation-preview foundation-up foundation-outputs foundation-destroy deploy-install deploy-typecheck deploy-preview deploy-up deploy-refresh deploy-outputs deploy-ssm deploy-destroy
