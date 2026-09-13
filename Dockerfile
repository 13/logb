# --- frontend ---
FROM node:26-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
# The build copies sources, not `.git`, so Settings > About can only show a commit when one is
# passed in: `docker build --build-arg LOGB_BUILD_COMMIT=$(git rev-parse HEAD) .`
ARG LOGB_BUILD_COMMIT=""
RUN npm run build

# --- backend (static musl binary) ---
FROM rust:1-alpine AS backend
RUN apk add --no-cache musl-dev ca-certificates
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY --from=frontend /app/frontend/dist ./frontend/dist
RUN cargo build --release --locked
# An empty, correctly owned /data to seed the volume with -- scratch has no shell to mkdir in.
RUN mkdir -p /seed/data

# --- runtime ---
FROM scratch
COPY --from=backend /app/target/release/logb /logb
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
