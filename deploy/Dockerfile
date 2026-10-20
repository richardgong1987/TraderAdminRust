# syntax=docker/dockerfile:1

# Build stage: compile the release binary.
FROM rust:1.98-slim-trixie AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# build.rs reads application.json5 to pick the database driver at compile time.
COPY Cargo.toml Cargo.lock build.rs application.json5 ./
COPY src ./src
# The cache mounts keep downloaded crates and compiled dependencies between builds, so a deploy only
# recompiles what changed. target/ lives in the cache, so the binary is copied out in the same step.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/abs_admin /usr/local/bin/abs_admin

# Runtime stage: the binary plus the files it reads from its working directory.
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3t64 \
    && rm -rf /var/lib/apt/lists/*
# Uploads and logs go to /app/target. The app_data volume is mounted there; a new volume takes its
# owner from this directory, so the non-root user can write to it.
RUN useradd --system --uid 10001 app \
    && mkdir -p /app/target \
    && chown app /app/target
WORKDIR /app
COPY --from=build /usr/local/bin/abs_admin /usr/local/bin/abs_admin
COPY application.json5 ./
COPY dist ./dist
USER app
EXPOSE 8000
CMD ["abs_admin"]
