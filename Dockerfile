# Needs BuildKit (`docker buildx`, the default builder in Docker Desktop and the docker-ce
# packages): the build stages name `$BUILDPLATFORM`, which the legacy builder does not set.
#
# Base images are pinned by digest, with the tag kept for the reader and for Dependabot. The rust
# tag follows rust-toolchain.toml.

# --- frontend ---
# The bundle is the same bytes for every architecture, so it is built once, natively, rather than
# under emulation for each target.
FROM --platform=$BUILDPLATFORM node:26-alpine@sha256:0b36e8c136b94cd4fcf02188228e76c31ad5872eef3fec8cbd2eee500cfd9e80 AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
# The build copies sources, not `.git`, so Settings > About can only show a commit when one is
# passed in: `docker build --build-arg LOGB_BUILD_COMMIT=$(git rev-parse HEAD) .`
ARG LOGB_BUILD_COMMIT=""
RUN npm run build

# --- dependency manifest ---
# Cargo.toml and Cargo.lock with LogB's own version replaced by a constant, so that bumping it
# for a release leaves the files -- and so the cached dependency layer below -- unchanged. Only
# the first `version =` in Cargo.toml is the package's (dependencies spell theirs inline), and
# in Cargo.lock only the one right after `name = "logb"`.
FROM --platform=$BUILDPLATFORM rust:1.98.1-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f AS manifest
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# `1,/re/` rather than GNU's `0,/re/`: this image's sed is BusyBox's, which does not know the
# latter and left Cargo.toml unchanged. The greps fail the build if either file was missed,
# rather than letting `cargo --locked` fail later with a lock file it wants to update.
RUN sed -i '1,/^version = /s/^version = .*/version = "0.0.0"/' Cargo.toml \
 && sed -i '/^name = "logb"$/{n;s/^version = .*/version = "0.0.0"/}' Cargo.lock \
 && grep -q '^version = "0.0.0"$' Cargo.toml \
 && grep -A1 '^name = "logb"$' Cargo.lock | grep -q '^version = "0.0.0"$'

# --- backend (static musl binary) ---
# Runs on the build machine and cross-compiles to the target, instead of compiling under QEMU,
# which for a release build with LTO is the difference between minutes and an hour. A native
# build (target == build architecture) uses plain cargo; a cross build links with zig, which
# also serves as the C cross compiler for the bundled SQLite and the TLS crates.
FROM --platform=$BUILDPLATFORM rust:1.98.1-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f AS backend
ARG BUILDARCH
ARG TARGETARCH
RUN apk add --no-cache musl-dev ca-certificates \
 && if [ "$TARGETARCH" != "$BUILDARCH" ]; then apk add --no-cache zig cargo-zigbuild; fi
RUN case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-musl ;; \
      arm64) triple=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && echo "$triple" > /target-triple \
 && rustup target add "$triple"
WORKDIR /app
# Dependencies first, against placeholder sources, so this layer depends on the dependency list
# alone: a change to LogB's own code -- or a release, which changes nothing but its version --
# reuses the cached layer (the release workflow caches layers with `cache-to: type=gha,mode=max`)
# and recompiles one crate instead of every dependency.
COPY --from=manifest /app/Cargo.toml /app/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
 && triple=$(cat /target-triple) \
 && if [ "$TARGETARCH" = "$BUILDARCH" ]; then build=build; else build=zigbuild; fi \
 && cargo "$build" --release --locked --target "$triple" \
 && rm -rf src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY --from=frontend /app/frontend/dist ./frontend/dist
# `touch`, so cargo sees the real sources as newer than the placeholders it built above even
# when the checkout's timestamps say otherwise.
RUN touch src/main.rs src/lib.rs \
 && triple=$(cat /target-triple) \
 && if [ "$TARGETARCH" = "$BUILDARCH" ]; then build=build; else build=zigbuild; fi \
 && cargo "$build" --release --locked --target "$triple" \
 && cp "target/$triple/release/logb" /logb
# An empty, correctly owned /data to seed the volume with -- scratch has no shell to mkdir in.
RUN mkdir -p /seed/data

# --- runtime ---
FROM scratch
COPY --from=backend /logb /logb
# scratch has no trust store, so an HTTPS LOGB_NOTIFY_URL would fail to verify.
COPY --from=backend /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=backend --chown=65532:65532 /seed/data /data
# Nothing here needs root. A named volume inherits this ownership; a bind mount keeps the
# host's, so chown the host directory to 65532 before mounting one.
USER 65532:65532
ENV LOGB_DATA_DIR=/data LOGB_BIND=0.0.0.0 LOGB_PORT=8080
VOLUME ["/data"]
EXPOSE 8080
# No shell and no curl in the image, so the binary probes itself.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s CMD ["/logb", "--healthcheck"]
ENTRYPOINT ["/logb"]
