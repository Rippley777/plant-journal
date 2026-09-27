# Build on the requested Linux platform (ACR command uses linux/amd64).
FROM rust:1.86.0-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY .cargo/config.toml .cargo/config.toml
COPY src/ src/
COPY static/ static/
COPY templates/ templates/
COPY migrations/ migrations/
COPY resources/ resources/
COPY tests/ tests/
COPY deploy/config.cloud.toml deploy/config.cloud.toml
RUN cargo test --locked && cargo build --locked --release --bin plant-journal

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata libgcc-s1 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 plant-journal \
    && useradd --uid 10001 --gid plant-journal --no-create-home --shell /usr/sbin/nologin plant-journal \
    && mkdir -p /app /home/plant-journal \
    && chown plant-journal:plant-journal /home/plant-journal
WORKDIR /app
COPY --from=build /build/target/release/plant-journal /usr/local/bin/plant-journal
COPY deploy/config.cloud.toml /app/config.cloud.toml
ENV PLANT_CONFIG=/app/config.cloud.toml \
    RUST_LOG=plant_journal=info,tower_http=info
USER 10001:10001
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/plant-journal"]
