# Containerfile for RamShield.
# Two-stage build; final image is a static binary on distroless.
#
#   docker build -t ghcr.io/grep999/ramshield:0.2.0 .
#   docker push ghcr.io/grep999/ramshield:0.2.0
#
# The image is non-root (uid 65532), read-only rootfs friendly, and exposes
# only the IPC and dashboard ports declared in deploy/k8s/deployment.yaml.

# ---- builder ----
FROM rust:1.85-bookworm AS builder
WORKDIR /build

# Cache dep layer first.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN mkdir -p src \
    && echo "fn main() {}" > src/main.rs \
    && echo "fn main() {}" > src/cli.rs \
    && cargo build --release --locked -F full \
    && rm -rf src

# Now copy the real source and rebuild (deps cached).
COPY src ./src
RUN touch src/main.rs src/cli.rs && cargo build --release --locked -F full \
    && strip target/release/ramshield

# ---- runtime ----
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /build/target/release/ramshield /usr/local/bin/ramshield
USER 65532:65532
EXPOSE 7890 9999
ENTRYPOINT ["/usr/local/bin/ramshield"]
