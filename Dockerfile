# Build order per ARCHITECTURE.md: UI first, then the daemon embeds ui/dist at compile time.
# Base tags are pinned (M5 hardening) for reproducible builds.

# --- Stage 1: UI ---
FROM node:20-slim AS ui
WORKDIR /app/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
RUN npm run build

# --- Stage 2: daemon (release) ---
FROM rust:1.94-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY core/ core/
COPY daemon/ daemon/
COPY --from=ui /app/ui/dist ui/dist
# --locked: build exactly the committed Cargo.lock (reproducible).
RUN cargo build --release --locked -p phonehome-daemon

# --- Stage 3: runtime (one artifact, D-006) ---
FROM debian:bookworm-slim
RUN useradd -r -s /usr/sbin/nologin phonehome \
    && mkdir -p /data \
    && chown phonehome /data \
    && chmod 700 /data
COPY --from=build /app/target/release/phonehome-daemon /usr/local/bin/phonehome-daemon
USER phonehome
ENV PHONEHOME_PORT=8480
ENV PHONEHOME_DB=/data/phonehome.db
EXPOSE 8480
VOLUME ["/data"]
# Probes /api/health via the daemon itself — no curl/wget in the slim image, and
# it works under compose's read_only root filesystem.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD ["phonehome-daemon", "--healthcheck"]
CMD ["phonehome-daemon"]
