# Build order per ARCHITECTURE.md: UI first, then the daemon embeds ui/dist at compile time.

# --- Stage 1: UI ---
FROM node:20-slim AS ui
WORKDIR /app/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
RUN npm run build

# --- Stage 2: daemon (release) ---
FROM rust:1-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY core/ core/
COPY daemon/ daemon/
COPY --from=ui /app/ui/dist ui/dist
RUN cargo build --release -p phonehome-daemon

# --- Stage 3: runtime (one artifact, D-006) ---
FROM debian:bookworm-slim
RUN useradd -r -s /usr/sbin/nologin phonehome \
    && mkdir -p /data \
    && chown phonehome /data
COPY --from=build /app/target/release/phonehome-daemon /usr/local/bin/phonehome-daemon
USER phonehome
ENV PHONEHOME_PORT=8480
EXPOSE 8480
VOLUME ["/data"]
CMD ["phonehome-daemon"]
