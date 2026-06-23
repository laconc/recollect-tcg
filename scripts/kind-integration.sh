#!/usr/bin/env bash
# Ephemeral kind cluster integration test — used by CI and runnable locally.
# Creates a throwaway cluster, deploys the server, smoke-tests it, tears down.
set -euo pipefail
CLUSTER=recollect-it-$RANDOM
trap 'kind delete cluster --name "$CLUSTER" || true' EXIT

docker build -f Dockerfile -t recollect-server:it .
kind create cluster --name "$CLUSTER" --config deploy/kind/kind.yaml --wait 120s
kind load docker-image recollect-server:it --name "$CLUSTER"

kubectl kustomize deploy/k8s/overlays/local \
  | sed 's/recollect-server:dev/recollect-server:it/' \
  | kubectl apply -f -
kubectl -n recollect rollout status deploy/recollect-server --timeout=120s

# Smoke: health + match creation through the NodePort mapping.
for i in $(seq 1 30); do curl -fsS localhost:8080/healthz && break || sleep 2; done
MATCH_JSON=$(curl -fsS -X POST localhost:8080/matches)
echo "$MATCH_JSON" | grep -q match_id
echo "kind integration: OK -> $MATCH_JSON"
# M1 extends this: a bot Job plays full matches over the WebSocket, then we
# kill the pod mid-match and assert snapshot-resume hash equality (chaos test).
