# --- frontend ---
FROM node:26-alpine AS frontend
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend ./
RUN npm run build

# --- backend (static musl binary) ---
FROM rust:1-alpine AS backend
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY --from=frontend /app/frontend/dist ./frontend/dist
RUN cargo build --release --locked

# --- runtime ---
FROM scratch
COPY --from=backend /app/target/release/memto /memto
ENV MEMTO_DATA_DIR=/data MEMTO_BIND=0.0.0.0 MEMTO_PORT=8080
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["/memto"]
