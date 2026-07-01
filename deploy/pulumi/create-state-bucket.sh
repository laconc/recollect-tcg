#!/usr/bin/env bash
#
# Create + harden the S3 bucket that backs Pulumi's self-managed state, then `pulumi login` to it.
# This is the ONE-TIME prerequisite before FOUNDATION/PLATFORM (the alternative to Pulumi Cloud or
# `pulumi login --local`). Run it once with a short-lived ADMIN AWS session (the same SSO login the
# Pulumi stacks use).
#
# Idempotent: safe to re-run — the bucket is created only if absent, and every setting converges.
#
# The Pulumi STATE backend is independent of the DEPLOY region: this bucket can live in ANY region —
# the `?region=` pin on the login URL (step 8) makes Pulumi address it correctly regardless of the
# ambient AWS default. The default co-locates it with the box's us-east-2 for simplicity. It only
# stores Pulumi's state objects.
#
# Knobs (env):
#   BUCKET       state bucket name       (default: recollect-tcg-pulumi-state-5c61fb)
#   REGION       bucket region           (default: us-east-2)
#   ENVIRONMENT  Environment tag value   (default: production — matches the Pulumi stacks' default)
#   REPOSITORY   Repository tag value    (owner/repo or repo URL; unset ⇒ tag omitted, never hardcoded)
#   NO_LOGIN     set to 1 to skip the final `pulumi login` (just create/harden the bucket)
set -euo pipefail

BUCKET="${BUCKET:-recollect-tcg-pulumi-state-5c61fb}"
REGION="${REGION:-us-east-2}"
ENVIRONMENT="${ENVIRONMENT:-production}"
REPOSITORY="${REPOSITORY:-}"

# The self-managed S3 backend encrypts stack secrets with a passphrase — require it UP FRONT so the
# whole deploy flow (every `pulumi` call after this) inherits it, rather than failing mid-stack-init.
if [ -z "${PULUMI_CONFIG_PASSPHRASE:-}" ]; then
  echo "ERROR: PULUMI_CONFIG_PASSPHRASE is not set in this shell." >&2
  echo "  The S3 backend needs it to encrypt stack secrets. Export it first (and save it in your" >&2
  echo "  password manager — losing it loses every encrypted secret in state):" >&2
  echo "    export PULUMI_CONFIG_PASSPHRASE='<a-strong-passphrase>'" >&2
  echo "  (Keyless alternative: skip the passphrase and pass --secrets-provider awskms://<key-arn>" >&2
  echo "   at 'pulumi stack init' instead — then drop this check.)" >&2
  exit 1
fi

echo "==> Pulumi state backend: s3://${BUCKET}  (${REGION})"

# 1. Create the bucket — only if it doesn't already exist (and we own it).
if aws s3api head-bucket --bucket "$BUCKET" 2>/dev/null; then
  echo "    bucket already exists — reconfiguring in place"
else
  aws s3 mb "s3://${BUCKET}" --region "$REGION"
fi

# 2. Versioning — state recovery. Pulumi leans on object versions to roll a bad `up` back.
aws s3api put-bucket-versioning --bucket "$BUCKET" \
  --versioning-configuration Status=Enabled

# 3. Default encryption: SSE-S3 (AES-256) + bucket keys (KMS-free at-rest encryption, no per-object key cost).
aws s3api put-bucket-encryption --bucket "$BUCKET" \
  --server-side-encryption-configuration '{
    "Rules": [{
      "ApplyServerSideEncryptionByDefault": { "SSEAlgorithm": "AES256" },
      "BucketKeyEnabled": true
    }]
  }'

# 4. Block ALL public access.
aws s3api put-public-access-block --bucket "$BUCKET" \
  --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true

# 5. Disable ACLs — IAM-only access control (the modern S3 default; no ACL surface).
aws s3api put-bucket-ownership-controls --bucket "$BUCKET" \
  --ownership-controls 'Rules=[{ObjectOwnership=BucketOwnerEnforced}]'

# 6. Deny any non-TLS access (defense in depth — Pulumi always talks HTTPS).
POLICY="$(mktemp)"
trap 'rm -f "$POLICY"' EXIT
cat > "$POLICY" <<EOF
{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "DenyInsecureTransport",
    "Effect": "Deny",
    "Principal": "*",
    "Action": "s3:*",
    "Resource": [
      "arn:aws:s3:::${BUCKET}",
      "arn:aws:s3:::${BUCKET}/*"
    ],
    "Condition": { "Bool": { "aws:SecureTransport": "false" } }
  }]
}
EOF
aws s3api put-bucket-policy --bucket "$BUCKET" --policy "file://${POLICY}"

# 7. Tag — mirror the Pulumi commonTags standard (deploy/README.md "Tagging") so the state bucket
#    slices the same way in the console / Cost Explorer as every stack resource. put-bucket-tagging is
#    a full replace, so re-running converges (idempotent). Stack=state-backend (NOT foundation/platform):
#    this bucket UNDERLIES both stacks and is bootstrap-created here, never part of a `pulumi up` —
#    don't `pulumi destroy` it. Repository follows the stacks' rule: config-driven, never hardcoded
#    (set REPOSITORY=owner/repo to add it). Name is the per-resource console display name.
TAGS="{Key=Project,Value=recollect},{Key=Environment,Value=${ENVIRONMENT}},{Key=ManagedBy,Value=pulumi},{Key=Stack,Value=state-backend},{Key=Name,Value=${BUCKET}}"
if [ -n "$REPOSITORY" ]; then
  TAGS="${TAGS},{Key=Repository,Value=${REPOSITORY}}"
fi
aws s3api put-bucket-tagging --bucket "$BUCKET" --tagging "TagSet=[${TAGS}]"

echo "==> bucket ready + hardened + tagged (versioning · AES-256 · no public access · IAM-only · TLS-only)"

# 8. Point Pulumi at it. Both stacks then read/write their state here.
#    Pin ?region= so Pulumi's DIY s3:// backend (gocloud.dev) addresses the bucket's region directly.
#    Without it the backend resolves region from the ambient AWS config, and if that differs from the
#    bucket's region S3 answers 301 PermanentRedirect (the aws CLI auto-follows that redirect; gocloud
#    does not). Pinning it keeps the login robust no matter where BUCKET/REGION place the bucket.
BACKEND="s3://${BUCKET}?region=${REGION}"
if [ "${NO_LOGIN:-0}" = "1" ]; then
  echo "    NO_LOGIN=1 — skipped pulumi login; run it yourself:  pulumi login '${BACKEND}'"
else
  pulumi login "${BACKEND}"
  echo ""
  echo "==> Logged in to the S3 backend. PULUMI_CONFIG_PASSPHRASE is set, so stack secrets encrypt"
  echo "    with it (keep it safe — it is the only key to them)."
  echo "    NEXT: 'make foundation-install' then 'make foundation-up' (or 'pulumi stack init prod' in"
  echo "    deploy/pulumi/foundation) — the passphrase is already in this shell."
fi
