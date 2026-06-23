#!/usr/bin/env bash
# Recollect — EC2 cloud-init user-data (the §10.1 lean launch host; the PLATFORM stack).
#
# Runs ONCE on first boot (Amazon Linux 2023). It installs Docker + the compose plugin, clones
# the repo at a pinned ref (for the compose files + the site build), writes the secrets .env, logs
# in to ECR with the instance role, PULLS the server image FOUNDATION's CI pushed (production does
# NOT build the server on this 1 GB box), and brings up the deploy stack:
#   postgres + site-builder + recollect-server + cloudflared + lgtm + node-exporter
#     (no inbound ports; the tunnel dials out — and Grafana is gated by Cloudflare Access)
# It also installs the lightweight CloudWatch agent (the §11 out-of-band box-health metrics).
#
# Pulumi renders the @@PLACEHOLDERS@@ below from config/secrets and ships this as the instance's
# user-data. In Pulumi state the rendered blob is a tracked SECRET (encrypted). On the box it is
# the EBS root volume (encrypted at rest) and is also retrievable from the instance metadata
# service by anything ON the box — which is why IMDSv2 is REQUIRED with hop-limit 1 (index.ts), so
# an SSRF in the app can't reach 169.254.169.254 to read it. The script is idempotent: re-running
# it (e.g. via `cloud-init clean && reboot`) is safe.
set -euo pipefail
exec > >(tee -a /var/log/recollect-bootstrap.log) 2>&1
echo "=== recollect bootstrap $(date -u +%FT%TZ) ==="

# --- Rendered by Pulumi (do NOT commit real values) -------------------------------------------
REPO_URL="@@REPO_URL@@"          # e.g. https://github.com/yourorg/recollect.git
GIT_REF="@@GIT_REF@@"            # a pinned commit SHA or tag — never a moving branch
IMAGE_REF="@@IMAGE_REF@@"        # the ECR server image to PULL, e.g. <acct>.dkr.ecr.<region>…/recollect-server:<gitRef>
SITE_ORIGIN="@@SITE_ORIGIN@@"    # e.g. https://your-domain.com
TUNNEL_TOKEN="@@TUNNEL_TOKEN@@"  # Cloudflare Tunnel connector token (Pulumi-created)
POSTGRES_PASSWORD="@@POSTGRES_PASSWORD@@"
OTEL_EXPORTER_OTLP_ENDPOINT="@@OTEL_ENDPOINT@@"  # empty ⇒ on-box lgtm (compose default); set ⇒ off-box
CF_BEACON_TOKEN="@@CF_BEACON_TOKEN@@"            # may be empty ⇒ no analytics beacon
SWAP_SIZE_GB="@@SWAP_SIZE_GB@@"                  # swap file size (GiB) on /data; default 4
OBS_GRAFANA_DOMAIN="@@OBS_GRAFANA_DOMAIN@@"      # bare domain; compose builds https://grafana.<domain>
# ----------------------------------------------------------------------------------------------

APP_DIR=/opt/recollect
COMPOSE_FILE="$APP_DIR/deploy/compose/docker-compose.deploy.yml"

echo "--> installing docker + git"
dnf -y update
dnf -y install docker git
# The compose v2 plugin (so `docker compose` works, not the legacy `docker-compose`).
mkdir -p /usr/libexec/docker/cli-plugins
COMPOSE_VERSION=v2.39.4
ARCH="$(uname -m)"
curl -fsSL \
  "https://github.com/docker/compose/releases/download/${COMPOSE_VERSION}/docker-compose-linux-${ARCH}" \
  -o /usr/libexec/docker/cli-plugins/docker-compose
chmod +x /usr/libexec/docker/cli-plugins/docker-compose
systemctl enable --now docker

echo "--> fetching the app at ${GIT_REF}"
if [ ! -d "$APP_DIR/.git" ]; then
  git clone "$REPO_URL" "$APP_DIR"
fi
git -C "$APP_DIR" fetch --all --tags --prune
git -C "$APP_DIR" checkout --force "$GIT_REF"

echo "--> writing the secrets .env (root-only)"
# Lives next to the repo; the compose file reads it. 0600 + root-owned so only root reads secrets.
ENV_FILE="$APP_DIR/.env"
umask 077
cat > "$ENV_FILE" <<EOF
IMAGE_REF=${IMAGE_REF}
POSTGRES_PASSWORD=${POSTGRES_PASSWORD}
TUNNEL_TOKEN=${TUNNEL_TOKEN}
SITE_ORIGIN=${SITE_ORIGIN}
OTEL_EXPORTER_OTLP_ENDPOINT=${OTEL_EXPORTER_OTLP_ENDPOINT}
CF_BEACON_TOKEN=${CF_BEACON_TOKEN}
GIT_REF=${GIT_REF}
SOURCE_URL=${REPO_URL}
RUST_LOG=info
OBS_GRAFANA_DOMAIN=${OBS_GRAFANA_DOMAIN}
OBS_DATA_DIR=/data/observability
EOF
umask 022

# --- DURABLE DATA VOLUME: mount the separate EBS volume at /data, then per-service subdirs --------
# This is the crux of durability and the RISKY step: the data volume (attached at /dev/sdf by
# Pulumi) outlives the box, so on a recreate it ALREADY HOLDS the box's state (the match journal +
# accounts under /data/postgres; a light self-hosted Grafana + metrics TSDB will share /data soon).
# We mount it GENERICALLY at /data and create a per-service subdir below — and we FORMAT it ONLY
# when it is brand-new (no filesystem). Reformatting an already-formatted volume would wipe the data
# on EVERY boot — a durability-destroying bug — so the format is strictly conditional on "no
# filesystem present". The whole block is idempotent.
DATA_DEV=/dev/sdf
DATA_MNT=/data
echo "--> preparing the durable data volume"
# On Nitro (t3) the volume Pulumi attached at /dev/sdf surfaces as an NVMe node with an
# UNPREDICTABLE name (/dev/nvme1n1, /dev/nvme2n1, …) — the BSD-style /dev/sdf name is not honoured.
# Resolve to the real node robustly, trying the stable identifiers in order and giving the
# attachment time to settle (it can lag the first boot):
#   1. the literal /dev/sdf (AL2023 sometimes installs it as a symlink to the NVMe node),
#   2. an /dev/disk/by-id/ symlink whose name ends in the requested device (…-sdf), and
#   3. the lone non-root whole NVMe disk (the only other EBS volume on this box is the data one).
# Resolution is what we WAIT on — not the literal path — so an NVMe-only AMI doesn't waste the wait.
resolve_data_dev() {
  local real
  real="$(readlink -f "$DATA_DEV" 2>/dev/null || true)"
  if [ -n "$real" ] && [ -b "$real" ]; then echo "$real"; return; fi
  for link in /dev/disk/by-id/*sdf; do
    if [ -e "$link" ]; then
      real="$(readlink -f "$link")"
      if [ -b "$real" ]; then echo "$real"; return; fi
    fi
  done
  # The root disk is whatever / is mounted from, minus any trailing partition (…p1 → the disk).
  local root_disk
  root_disk="$(lsblk -dpno PKNAME "$(findmnt -no SOURCE /)" 2>/dev/null || true)"
  [ -z "$root_disk" ] && root_disk="$(findmnt -no SOURCE / | sed 's/p\?[0-9]*$//')"
  # Defense-in-depth: if we cannot identify the root disk, DO NOT guess the data disk. An empty
  # exclusion would let `grep -vx ""` keep the root disk in the candidate list, so a degenerate
  # topology could pick root as "the other disk". The FSTYPE guard below would still refuse to
  # reformat a root that already carries a filesystem, but bailing here is the belt to that braces:
  # an unresolved data device aborts the whole block (the caller exits) rather than mount the wrong one.
  [ -z "$root_disk" ] && return
  real="$(lsblk -dpno NAME,TYPE | awk '$2=="disk"{print $1}' | grep -vx "$root_disk" | head -n1)"
  [ -n "$real" ] && [ -b "$real" ] && echo "$real"
}
DATA_REAL=""
for _ in $(seq 1 30); do
  DATA_REAL="$(resolve_data_dev)"
  if [ -n "$DATA_REAL" ]; then break; fi
  echo "    waiting for the data volume ($DATA_DEV) to attach…"; sleep 2
done
echo "    data device resolved to: ${DATA_REAL:-<none>}"
if [ -z "$DATA_REAL" ] || [ ! -b "$DATA_REAL" ]; then
  echo "!!! could not resolve the data volume at $DATA_DEV — aborting before it could be misused" >&2
  exit 1
fi
# Does it ALREADY carry a filesystem? `blkid` prints the type (and exits 0) iff one exists; a brand
# -new volume prints nothing (exit 2). lsblk -f corroborates. We FORMAT ONLY when there is none.
FSTYPE="$(blkid -o value -s TYPE "$DATA_REAL" 2>/dev/null || true)"
if [ -z "$FSTYPE" ]; then
  echo "    no filesystem on $DATA_REAL — formatting ext4 (first boot for this volume)"
  mkfs.ext4 -L recollect-data "$DATA_REAL"
else
  echo "    existing $FSTYPE filesystem on $DATA_REAL — MOUNTING as-is (never reformatting)"
fi
# Persist by UUID (stable across the NVMe name churn) with nofail so a missing/late volume can
# never wedge the boot. Mount now if not already mounted.
DATA_UUID="$(blkid -o value -s UUID "$DATA_REAL")"
mkdir -p "$DATA_MNT"
if ! grep -q "UUID=${DATA_UUID}" /etc/fstab; then
  echo "UUID=${DATA_UUID} ${DATA_MNT} ext4 defaults,nofail 0 2" >> /etc/fstab
fi
if ! mountpoint -q "$DATA_MNT"; then
  mount "$DATA_MNT"
fi
# Per-service subdirs on the durable volume. Postgres' data dir: the alpine postgres image runs as
# UID:GID 70:70 — own it so the container can write. (-p is a no-op if it already exists from a
# previous boot.)
mkdir -p "$DATA_MNT/postgres"
chown -R 70:70 "$DATA_MNT/postgres"
# The self-hosted observability stack's data dir: Grafana state + the metrics/logs/traces stores
# (bind-mounted to the lgtm container's /data). The otel-lgtm image runs as root inside the
# container, so no host-side chown is needed — just ensure the dir exists on the durable volume.
mkdir -p "$DATA_MNT/observability"
echo "    data volume ready at $DATA_MNT (postgres: $DATA_MNT/postgres, observability: $DATA_MNT/observability)"
# ----------------------------------------------------------------------------------------------

# --- SWAP FILE on the durable /data volume (RAM headroom for the 1 GB box) ---------------------
# The box is 1 GB; the self-hosted LGTM observability stack plus the server + Postgres want more
# headroom, and the first `docker build` is memory-hungry. A swap file on /data (NOT the root,
# NOT a separate device) is the safety net. It MUST come after /data is mounted. Idempotent: created
# ONLY if absent; vm.swappiness is set LOW (10) so swap is a fallback, not a default. /data being a
# real filesystem (ext4) means fallocate/swapon work normally.
SWAP_FILE="$DATA_MNT/swapfile"
echo "--> ensuring ${SWAP_SIZE_GB}G swap at $SWAP_FILE"
if ! swapon --show=NAME --noheadings 2>/dev/null | grep -qx "$SWAP_FILE"; then
  if [ ! -f "$SWAP_FILE" ]; then
    # fallocate is instant; fall back to dd if the fs rejects it (ext4 supports fallocate, so this is
    # belt-and-braces). The file must be 0600 + root-owned or mkswap/swapon refuse it.
    if ! fallocate -l "${SWAP_SIZE_GB}G" "$SWAP_FILE" 2>/dev/null; then
      dd if=/dev/zero of="$SWAP_FILE" bs=1M count=$((SWAP_SIZE_GB * 1024)) status=none
    fi
    chmod 600 "$SWAP_FILE"
    mkswap "$SWAP_FILE"
  fi
  swapon "$SWAP_FILE"
fi
# Persist across reboots. nofail so a missing /data (volume detached) can never wedge boot. Use the
# path (a swap file has no UUID line in fstab the way a filesystem does).
if ! grep -q "^${SWAP_FILE} " /etc/fstab; then
  echo "${SWAP_FILE} none swap sw,nofail 0 0" >> /etc/fstab
fi
# Swap is a safety net, not a hot path: bias the kernel toward RAM (default swappiness is 60).
sysctl -w vm.swappiness=10
if [ ! -f /etc/sysctl.d/99-recollect-swappiness.conf ]; then
  echo "vm.swappiness=10" > /etc/sysctl.d/99-recollect-swappiness.conf
fi
echo "    swap ready ($(swapon --show=NAME,SIZE --noheadings | tr '\n' ' '))"
# ----------------------------------------------------------------------------------------------

# --- CloudWatch agent: the OUT-OF-BAND host metrics (mem/swap/disk) the §11 alarms watch ---------
# The in-box Grafana/node-exporter dashboard can't alarm on its own outage, so the lightweight
# CloudWatch agent publishes a few custom host metrics to CloudWatch, where Pulumi's alarms page by
# email even if the box (or the compose stack) is wedged. AL2023 ships the agent as a dnf package.
# Config is the COMMITTED file in the cloned repo (no secrets in it). Idempotent: re-running just
# re-applies the same config. Failure here is non-fatal — the box + game must come up regardless.
echo "--> installing + starting the CloudWatch agent (out-of-band box health)"
CW_AGENT_CONFIG="$APP_DIR/deploy/compose/observability/cloudwatch-agent.json"
if dnf -y install amazon-cloudwatch-agent; then
  # `-c file:<cfg>` reads the local config; `-s` starts the agent under systemd. The agent resolves
  # ${aws:InstanceId} itself (it has the instance role) and pushes on the 300s interval in the cfg.
  if /opt/aws/amazon-cloudwatch-agent/bin/amazon-cloudwatch-agent-ctl \
      -a fetch-config -m ec2 -s -c "file:${CW_AGENT_CONFIG}"; then
    echo "    CloudWatch agent running (namespace Recollect/Host: mem/swap/disk every 300s)"
  else
    echo "    !! CloudWatch agent failed to start — alarms on custom metrics will read no-data" >&2
  fi
else
  echo "    !! amazon-cloudwatch-agent install failed — custom host alarms will read no-data" >&2
fi
# ----------------------------------------------------------------------------------------------

# --- ECR LOGIN: keyless `docker login` to pull the server image (the FOUNDATION/PLATFORM split) ---
# Production PULLS the server image FOUNDATION's CI built + pushed to ECR — the box does NOT compile
# Rust (the 1 GB micro can't afford it). The instance role grants ECR read-only, so the registry auth
# token comes from the role with NO stored credential. AL2023 ships the AWS CLI v2 + the SSM agent;
# `aws ecr get-login-password` mints a 12-hour token we pipe into `docker login`. IMAGE_REF is the full
# ECR ref (<acct>.dkr.ecr.<region>.amazonaws.com/<repo>:<tag>); derive the registry host + region from
# it so there is nothing else to render. Failure here is FATAL — without the image the stack can't run.
echo "--> logging in to ECR to pull the server image (${IMAGE_REF})"
ECR_REGISTRY="${IMAGE_REF%%/*}"                                    # …dkr.ecr.<region>.amazonaws.com
ECR_REGION="$(printf '%s' "$ECR_REGISTRY" | sed -n 's/.*\.dkr\.ecr\.\([^.]*\)\.amazonaws\.com$/\1/p')"
if [ -z "$ECR_REGION" ]; then
  echo "!!! could not parse the ECR region from IMAGE_REF='${IMAGE_REF}' — aborting" >&2
  exit 1
fi
# Retry briefly: the instance role / network can lag the very first boot.
for attempt in $(seq 1 5); do
  if aws ecr get-login-password --region "$ECR_REGION" \
       | docker login --username AWS --password-stdin "$ECR_REGISTRY"; then
    echo "    logged in to $ECR_REGISTRY"; break
  fi
  echo "    ECR login attempt ${attempt} failed; retrying…" >&2; sleep 5
  [ "$attempt" -eq 5 ] && { echo "!!! ECR login failed after 5 attempts — aborting" >&2; exit 1; }
done
# ----------------------------------------------------------------------------------------------

echo "--> docker compose pull (server from ECR) + up -d (build site, start postgres/cloudflared/lgtm/node-exporter)"
# --project-directory the COMPOSE-FILE dir (deploy/compose) so the relative bind-mounts
# (./observability/...) resolve to deploy/compose/observability where the files live, while the
# site-builder build context (../.. → repo root) still resolves too. This mirrors `make deploy-local`,
# whose project dir defaults to the compose-file dir; the .env is passed explicitly via --env-file.
#
# PULL the server image first (it comes from ECR — IMAGE_REF in .env), then `up -d` WITHOUT --build:
# the base compose gives `server` only an `image:`, so it is pulled, never built; the site-builder
# DOES build on-box (it bakes the per-deploy origin/beacon) and `up` builds it as needed. The
# production stack omits the BUILD overlay entirely — only local/smoke layer it (see the Makefile).
DEPLOY_COMPOSE=(docker compose
  --project-directory "$APP_DIR/deploy/compose"
  --env-file "$ENV_FILE"
  -f "$COMPOSE_FILE")
"${DEPLOY_COMPOSE[@]}" pull server
"${DEPLOY_COMPOSE[@]}" up -d

echo "--> installing the recollect updater unit (pull latest pinned ref + recreate)"
# A small helper so a redeploy is one `recollect-update <tag>` (or a CI ssh/ssm call): it re-points
# the box at a new pinned image tag (and the matching repo ref for the compose files + site), logs in
# to ECR, pulls, and recreates. Not timer-driven — deploys are intentional. Usage:
#   recollect-update <git-ref> [<image-ref>]
# <image-ref> defaults to the current .env IMAGE_REF with its tag swapped to <git-ref> (the usual
# case: CI tags the image with the SHA). Pass an explicit <image-ref> to override.
cat > /usr/local/bin/recollect-update <<'UPD'
#!/usr/bin/env bash
set -euo pipefail
APP_DIR=/opt/recollect
ENV_FILE="$APP_DIR/.env"
REF="${1:-$(git -C "$APP_DIR" rev-parse HEAD)}"
# Resolve the new image ref: explicit arg, else the .env IMAGE_REF with its :tag replaced by REF.
CUR_IMAGE="$(grep -E '^IMAGE_REF=' "$ENV_FILE" | cut -d= -f2-)"
NEW_IMAGE="${2:-${CUR_IMAGE%:*}:${REF}}"
git -C "$APP_DIR" fetch --all --tags --prune
git -C "$APP_DIR" checkout --force "$REF"
# Persist the new image ref into .env (sed in place; IMAGE_REF is the first line).
sed -i "s#^IMAGE_REF=.*#IMAGE_REF=${NEW_IMAGE}#" "$ENV_FILE"
# Keyless ECR login (instance role), then pull + recreate.
ECR_REGISTRY="${NEW_IMAGE%%/*}"
ECR_REGION="$(printf '%s' "$ECR_REGISTRY" | sed -n 's/.*\.dkr\.ecr\.\([^.]*\)\.amazonaws\.com$/\1/p')"
aws ecr get-login-password --region "$ECR_REGION" | docker login --username AWS --password-stdin "$ECR_REGISTRY"
docker compose --project-directory "$APP_DIR/deploy/compose" --env-file "$ENV_FILE" \
  -f "$APP_DIR/deploy/compose/docker-compose.deploy.yml" pull server
docker compose --project-directory "$APP_DIR/deploy/compose" --env-file "$ENV_FILE" \
  -f "$APP_DIR/deploy/compose/docker-compose.deploy.yml" up -d
docker image prune -f
UPD
chmod +x /usr/local/bin/recollect-update

echo "=== recollect bootstrap complete $(date -u +%FT%TZ) ==="
