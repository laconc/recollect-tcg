# syntax=docker/dockerfile:1
# Static musl build of the authoritative server, cached layer-by-layer with
# cargo-chef, shipped on distroless. Build context is the repo root; only app/
# is sent (see .dockerignore). OpenTelemetry is always compiled in and gated at
# runtime on OTEL_EXPORTER_OTLP_ENDPOINT (see telemetry.rs).
FROM rust:1.96-alpine AS chef
RUN apk add --no-cache build-base musl-dev && cargo install cargo-chef --locked
WORKDIR /src

# Plan: distil the dependency graph into recipe.json (changes only when deps do).
FROM chef AS planner
COPY app/ .
RUN cargo chef prepare --recipe-path recipe.json

# Build: cook the server's deps from the recipe (cached), then the real binary.
FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --profile dist --locked -p recollect-server --recipe-path recipe.json
COPY app/ .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --profile dist --locked -p recollect-server && \
    cp target/dist/recollect-server /usr/local/bin/recollect-server

# The musl binary is fully static, so distroless/static (ca-certs + nonroot) is
# all the runtime needs — no libc, no shell.
FROM gcr.io/distroless/static-debian12:nonroot AS runtime
ARG BUILD_DATE
ARG GIT_REF
ARG VERSION
ARG SOURCE_URL
LABEL org.opencontainers.image.title="recollect-server" \
      org.opencontainers.image.description="Authoritative server for Recollect, a deterministic two-player storytelling card game." \
      org.opencontainers.image.base.name="gcr.io/distroless/static-debian12:nonroot" \
      org.opencontainers.image.source="${SOURCE_URL}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.revision="${GIT_REF}" \
      org.opencontainers.image.version="${VERSION}"
COPY --from=builder /usr/local/bin/recollect-server /recollect-server
EXPOSE 8080
USER nonroot
ENTRYPOINT ["/recollect-server"]
