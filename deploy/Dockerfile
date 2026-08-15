FROM node:20-alpine AS frontend-build
WORKDIR /app/frontend
COPY frontend/package*.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

FROM rust:1.85-alpine AS backend-build
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY migrations/ migrations/
RUN cargo build --release

FROM alpine:3.20
RUN apk add --no-cache ca-certificates
WORKDIR /app
COPY --from=backend-build /app/target/release/crm-rb26164f /app/crm-rb26164f
COPY --from=backend-build /app/migrations /app/migrations
COPY --from=frontend-build /app/frontend/dist /app/frontend
EXPOSE 9092
CMD ["/app/crm-rb26164f"]
