# coa-web: the Manager's web interface and its backend in one image.
# Build context: the repository root.   docker build -f services/coa-web/Dockerfile -t coa-web:local .
#
# The service drives the host's Docker (the server's database, auth and world containers), so it runs with:
#   * the host's Docker socket mounted,
#   * the host network (it checks the servers' ports and talks to the RA console on 127.0.0.1),
#   * every server and game-data folder mounted at the SAME path as on the host, because these paths are handed to the
#     host's Docker as bind mounts.
# See deploy/docker-compose.yml.

FROM node:22-bookworm AS ui
WORKDIR /src
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY index.html vite.config.ts tsconfig.json .env.web ./
COPY src src
COPY modules modules
COPY addons addons
COPY schemas schemas
COPY changelog changelog
RUN npm run build:web

FROM rust:1-bookworm AS build
WORKDIR /src
# coa-core takes its version and edition from the Manager's workspace manifest, which also names its members.
COPY Cargo.toml ./
COPY crates crates
COPY src-tauri src-tauri
COPY services/coa-web services/coa-web
WORKDIR /src/services/coa-web
RUN cargo build --release --bin coa-web

# Only the Docker client and its build plugin (static binaries); the daemon is the host's.
FROM docker:27.5.1-cli AS docker-cli

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 curl tini \
    && rm -rf /var/lib/apt/lists/*
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=docker-cli /usr/local/libexec/docker/cli-plugins/docker-buildx /usr/local/libexec/docker/cli-plugins/docker-buildx
COPY --from=build /src/services/coa-web/target/release/coa-web /usr/local/bin/coa-web
COPY --from=ui /src/dist-web /usr/share/coa-web
RUN useradd --system --uid 10001 --user-group --create-home --home-dir /var/lib/coa-manager --shell /usr/sbin/nologin coa
ENV COA_WEB_STATIC=/usr/share/coa-web \
    COA_MANAGER_DATA_DIR=/var/lib/coa-manager \
    COA_WEB_BIND=127.0.0.1:8787
USER 10001:10001
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fs "http://${COA_WEB_BIND}/api/health" || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/coa-web"]
