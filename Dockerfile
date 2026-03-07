FROM rust:1.92.0-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY . .
RUN cargo build --release --package lemma-cli

FROM scratch
COPY --from=builder /build/target/release/lemma /usr/local/bin/lemma
WORKDIR /specs
ENTRYPOINT ["lemma"]
CMD ["--help"]
EXPOSE 8012
