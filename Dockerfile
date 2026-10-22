FROM node:24-bookworm-slim AS console
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --no-fund --no-audit
COPY ui/ ./
RUN npm run build

FROM rust:1.98-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && install -Dm755 target/release/broker_login /out/broker_login

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --home-dir /app --shell /usr/sbin/nologin broker
COPY --from=build /out/broker_login /usr/local/bin/broker_login
COPY --from=console /ui/dist /app/ui/dist
ENV CBL_HOME=/app \
    CBL_HTTP_ADDR=0.0.0.0:47440 \
    TZ=Asia/Kolkata
WORKDIR /app
RUN mkdir -p /app/data && chown -R broker:broker /app
USER broker
EXPOSE 47440
ENTRYPOINT ["/usr/local/bin/broker_login"]
