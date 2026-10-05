FROM rust:1.98.0-bookworm AS build
WORKDIR /app
COPY rust-toolchain.toml ./
COPY apps/api ./apps/api
COPY migrations ./migrations
COPY docs/ROADMAP.md ./docs/ROADMAP.md
WORKDIR /app/apps/api
RUN cargo build --release --locked --bins

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/apps/api/target/release/sver /usr/local/bin/sver
COPY --from=build /app/apps/api/target/release/sver-import-check /usr/local/bin/sver-import-check
COPY --from=build /app/apps/api/target/release/sver-admin /usr/local/bin/sver-admin
USER 65532:65532
CMD ["sver"]
