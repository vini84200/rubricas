# Binário único com a aplicação web embutida; a imagem final só tem ele.
FROM rust:1.97-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src src
COPY web web
COPY claude claude
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12
COPY --from=build /src/target/release/rubricas /usr/local/bin/rubricas
VOLUME /dados
EXPOSE 8080
ENTRYPOINT ["rubricas"]
CMD ["servir", "--dados", "/dados", "--endereco", "0.0.0.0:8080"]
