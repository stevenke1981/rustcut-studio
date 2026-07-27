FROM rust:1.97.1-bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --workspace

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg ca-certificates fonts-noto-cjk \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/rustcut-server /usr/local/bin/rustcut-server
COPY --from=builder /src/target/release/rustcut-cli /usr/local/bin/rustcut-cli
COPY --from=builder /src/target/release/rustcut-mcp /usr/local/bin/rustcut-mcp
ENV RUSTCUT_BIND=0.0.0.0:8787 RUSTCUT_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8787
ENTRYPOINT ["rustcut-server"]
