FROM rust:alpine AS builder

COPY . /build
WORKDIR /build
RUN apk update && \
    apk add musl-dev && \
    cargo build --release


FROM pandoc/latex:latest

COPY --from=builder /build/target/release/pandoc-bot-worker /root/

WORKDIR /root/
ENTRYPOINT [ "/root/pandoc-bot-worker" ]
