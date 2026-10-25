# Rust and runtime versions are fixed; Cargo.lock is mandatory.
FROM rust:1.94.0-bookworm@sha256:365468470075493dc4583f47387001854321c5a8583ea9604b297e67f01c5a4f AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY prompts ./prompts
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target \
    cargo build --locked --release --bin primeradiant && cp target/release/primeradiant /primeradiant
FROM build AS verify
COPY tests ./tests
COPY examples ./examples
COPY evaluation/phase3/english-draft-v1.txt ./evaluation/phase3/english-draft-v1.txt
COPY reports/evidence/phase-3/resumed-haiku/v52-v23-fresh-003-trace.json ./reports/evidence/phase-3/resumed-haiku/v52-v23-fresh-003-trace.json
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target \
    cargo fetch --locked && cargo test --locked --release --offline && cargo build --locked --release --offline --example compose_fixture --example container_embed_probe \
    && cp target/release/examples/compose_fixture /compose_fixture \
    && cp target/release/examples/container_embed_probe /container_embed_probe
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8
RUN sed -i 's|http://deb.debian.org/debian-security|http://snapshot.debian.org/archive/debian-security/20261020T213255Z|; s|http://deb.debian.org/debian$|http://snapshot.debian.org/archive/debian/20261020T213255Z|' /etc/apt/sources.list.d/debian.sources \
    && apt-get -o Acquire::Check-Valid-Until=false update \
    && apt-get install -y --no-install-recommends ca-certificates python3 && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 10001 radiant && useradd -u 10001 -g radiant -M radiant \
    && mkdir -p /data /credentials /backups && chown radiant:radiant /data /credentials /backups \
    && chmod 700 /data /credentials /backups
COPY --from=build /primeradiant /usr/local/bin/primeradiant
COPY deployment/ /opt/radiant/
USER 10001:10001
ENV HOME=/data
ENTRYPOINT ["/opt/radiant/entrypoint.sh"]
CMD ["serve"]
