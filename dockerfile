FROM rust:1.90-alpine AS builder

RUN apk add --no-cache musl-dev build-base
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app
COPY . .

RUN cargo build --release --target x86_64-unknown-linux-musl

FROM scratch

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/meshdock /meshdock

ENTRYPOINT ["/meshdock"]
