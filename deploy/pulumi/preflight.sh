#!/usr/bin/env bash
#
# Preflight for the Pulumi make targets (`make foundation-* / deploy-*`). Makes each lifecycle command
# SELF-SUFFICIENT: instead of failing mid-apply on a missing passphrase / unset config, it ensures
# everything Pulumi needs is present FIRST — prompting for whatever's missing — then `exec`s the command
# in the project dir. Running in ONE process is deliberate: a passphrase prompted here is exported into
# the same environment the `pulumi` call inherits (a separate preflight shell couldn't pass it on).
#
# Schema:
#   [STACK=prod] [ENVVARS="A B"] [REQUIRED="k1 k2"] [DEFAULTS="k=v ..."] \
#     preflight.sh <project-dir> <command> [args...]
#     STACK      stack to select/create                         (default: prod)
#     ENVVARS    space-separated secret env-var names to ensure (prompted silently if unset)
#     REQUIRED   space-separated config keys to ensure          (prompted if unset)
#                DEFAULT: every no-default key in the project's Pulumi.yaml (auto-derived, can't drift)
#     DEFAULTS   space-separated key=value config defaults       (set if unset)
#     <project-dir>   the Pulumi project to run in (deploy/pulumi/foundation | .../platform)
#     <command>...    the command exec'd after the checks pass  (usually `pulumi <verb>`)
#
# Examples (what the Makefile targets expand to — REQUIRED is omitted, so it's auto-derived from Pulumi.yaml):
#   # foundation-up — ensure passphrase, init/select prod, default region, prompt for githubRepo:
#   ENVVARS="PULUMI_CONFIG_PASSPHRASE" DEFAULTS="region=us-east-2" \
#     preflight.sh deploy/pulumi/foundation pulumi up
#
#   # deploy-up — also ensure the Cloudflare token; prompts for any of PLATFORM's required inputs:
#   ENVVARS="PULUMI_CONFIG_PASSPHRASE CLOUDFLARE_API_TOKEN" DEFAULTS="region=us-east-2" \
#     preflight.sh deploy/pulumi/platform pulumi up
#
#   # deploy-ssm — just need the passphrase + stack; exec an arbitrary command:
#   ENVVARS="PULUMI_CONFIG_PASSPHRASE" \
#     preflight.sh deploy/pulumi/platform bash -c 'eval "$(pulumi stack output ssmSession)"'
#
# Steps, in order:
#   1. ENVVARS  — each named env var must be set; prompt SILENTLY (these are secrets) for any that
#                 isn't and export it (e.g. PULUMI_CONFIG_PASSPHRASE, CLOUDFLARE_API_TOKEN).
#   2. AWS auth — a SOFT check (caller identity); a miss prints the `aws sso login` hint, never blocks.
#   3. STACK    — select it; create it with `pulumi stack init` if it doesn't exist (default: prod).
#   4. DEFAULTS — each `key=value` config set IF unset (optional knobs Pulumi defaults anyway, made
#                 explicit — e.g. region=us-east-2).
#   5. REQUIRED — each config key Pulumi has no default for is prompted for IF unset (e.g. githubRepo);
#                 an empty answer aborts rather than letting the apply fail later.
# then: exec the command (e.g. `pulumi up`) in <project-dir>.
set -euo pipefail

# Config keys with NO `default:` in a Pulumi.yaml — the ones Pulumi's stack-config validation requires
# you to set. DERIVED from the schema (not hardcoded) so the prompt list can never drift from it: add a
# no-default key to Pulumi.yaml and the preflight prompts for it automatically.
required_keys() {
  awk '
    /^config:/ { inconfig=1; next }
    inconfig && /^[^[:space:]]/ { inconfig=0 }
    inconfig && /^  [A-Za-z0-9_-]+:[A-Za-z0-9_]+:[[:space:]]*$/ {
      if (key != "" && !hasdef) print key
      line=$1; sub(/:$/,"",line); n=split(line,a,":"); key=a[n]; hasdef=0; next
    }
    inconfig && /^    default:/ { hasdef=1 }
    END { if (key != "" && !hasdef) print key }
  ' "$1"
}

DIR="$1"; shift
STACK="${STACK:-prod}"

cd "$DIR"

# 1. Needed secret env vars — prompt + export any that are missing.
for var in ${ENVVARS:-}; do
  if [ -z "${!var:-}" ]; then
    read -rsp "  $var is not set — enter it: " value; echo
    [ -n "$value" ] || { echo "ERROR: $var is required for this step." >&2; exit 1; }
    export "$var=$value"
  fi
done

# 2. AWS auth — soft check; Pulumi (and the S3 state backend) need creds for any cloud call.
if ! aws sts get-caller-identity >/dev/null 2>&1; then
  echo "  ! AWS creds not found — run 'aws sso login' (default profile) before a step that hits the cloud." >&2
fi

# 3. Stack — select, or create it if absent.
if ! pulumi stack select "$STACK" >/dev/null 2>&1; then
  echo "==> stack '$STACK' doesn't exist yet — creating it"
  pulumi stack init "$STACK"
fi

# 4. Optional defaults — set only if the key is unset.
for kv in ${DEFAULTS:-}; do
  key="${kv%%=*}"; val="${kv#*=}"
  if ! pulumi config get "$key" >/dev/null 2>&1; then
    pulumi config set "$key" "$val"
    echo "    config $key = $val  (default)"
  fi
done

# 5. Required config — every Pulumi.yaml key with no default (auto-derived, so the list can't drift
#    from the schema), unless REQUIRED was passed explicitly. Prompt for any that's still unset (the
#    step-4 defaults already ran, so keys like region won't re-prompt).
for key in ${REQUIRED:-$(required_keys Pulumi.yaml)}; do
  if ! pulumi config get "$key" >/dev/null 2>&1; then
    read -rp "  config '$key' is required — enter it: " val
    [ -n "$val" ] || { echo "ERROR: config '$key' is required." >&2; exit 1; }
    pulumi config set "$key" "$val"
  fi
done

echo "==> ${DIR#deploy/pulumi/} (stack $STACK): $*"
exec "$@"
