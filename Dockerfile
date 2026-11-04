# streaming-player: one image with the Rust server, the React web app, and yt-dlp
# (plus Deno, which yt-dlp needs for YouTube since 2025.11).

# 1. Web app
FROM node:24-bookworm-slim AS web
WORKDIR /web
RUN npm install -g pnpm@10.6.5
COPY src/web/package.json src/web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY src/web/ ./
RUN pnpm build

# 2. Server
FROM rust:1-trixie AS api
WORKDIR /api
COPY src/api/Cargo.toml src/api/Cargo.lock ./
COPY src/api/migrations ./migrations
COPY src/api/src ./src
RUN cargo build --release --locked

# 3. yt-dlp, pinned with its published SHA-256. The admin page (or
#    `streaming-player update-ytdlp`) can update it later; a rebuild with a newer
#    YTDLP_VERSION makes the update permanent.
FROM debian:trixie-slim AS ytdlp
ARG YTDLP_VERSION=2026.10.26
ARG YTDLP_SHA256_AMD64=767ba9dce7052aa196f8b12652f433d76de128ba168966fa84b6394d5e6c6795
ARG YTDLP_SHA256_ARM64=e87e9533e89fe5889abaa21f5114dfe7756d6aa6219b83551add04d48b18e483
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN case "${TARGETARCH:-amd64}" in \
        amd64) asset=yt-dlp_linux; sum="$YTDLP_SHA256_AMD64" ;; \
        arm64) asset=yt-dlp_linux_aarch64; sum="$YTDLP_SHA256_ARM64" ;; \
        *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && curl -fsSL -o /yt-dlp "https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}/${asset}" \
    && echo "${sum}  /yt-dlp" | sha256sum -c - \
    && chmod 755 /yt-dlp

# 4. Deno, the JavaScript runtime yt-dlp uses to solve YouTube's challenges
FROM denoland/deno:bin-2.9.8 AS deno

# 5. Runtime
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --home-dir /home/app --shell /usr/sbin/nologin app \
    && mkdir -p /app/data /opt/yt-dlp \
    && chown app /app/data /opt/yt-dlp
WORKDIR /app
COPY --from=deno /deno /usr/local/bin/deno
# Owned by the app user so the admin page can self-update it
COPY --from=ytdlp --chown=app /yt-dlp /opt/yt-dlp/yt-dlp
COPY --from=api /api/target/release/streaming-player /usr/local/bin/streaming-player
COPY --from=web /web/dist /app/web
ENV SP_BIND=0.0.0.0:7878 \
    SP_DATA_DIR=/app/data \
    SP_STATIC_DIR=/app/web \
    SP_YTDLP=/opt/yt-dlp/yt-dlp
VOLUME /app/data
EXPOSE 7878
USER app
CMD ["streaming-player"]
