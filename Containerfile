# Containerfile for RamShield.
# Two-stage build; final image contains the glibc-linked release binary on a minimal distroless runtime.
#
#   docker build -t ghcr.io/grep999/ramshield:${RAMSHIELD_VERSION} .
#   docker push ghcr.io/grep999/ramshield:${RAMSHIELD_VERSION}
#
# The image is non-root (uid 65532), read-only rootfs friendly, and exposes
# only the IPC and dashboard ports declared in deploy/k8s/deployment.yaml.

# ---- builder ----
# rust:nightly is required because the workspace uses edition = "2024"
# (unstable).  Pin to a specific nightly date in CI to avoid regressions.
FROM rustlang/rust:nightly-2026-08-29 AS builder
ARG RAMSHIELD_VERSION
WORKDIR /build

# Cache dep layer first.
COPY Cargo.toml Cargo.lock ./
COPY crates/*/Cargo.toml ./crates/
COPY crates/*/src ./crates/
RUN mkdir -p src \
    && cargo build --release --locked -F full \
    && rm -rf src

# ---- runtime ----
FROM gcr.io/distroless/cc-debian12:nonroot
ARG RAMSHIELD_VERSION
COPY --from=builder /build/target/release/ramshield /usr/local/bin/ramshield
USER 65532:65532
EXPOSE 7890 9999
ENTRYPOINT ["/usr/local/bin/ramshield"]

LABEL org.opencontainers.image.title="RamShield" \\
      org.opencontainers.image.description="Self-hosted Linux ingress defense daemon" \\
      org.opencontainers.image.version="${RAMSHIELD_VERSION}" \\
      org.opencontainers.image.source="https://github.com/grep999/ramshield" \\
      org.opencontainers.image.licenses="MIT"