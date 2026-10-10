# Binário único com a aplicação web embutida. Dois alvos:
#   servidor (padrão): só o binário, sobre distroless.
#   executor: o binário junto com um podman, que roda as checagens em contêineres aninhados.
FROM rust:1.97-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src src
COPY web web
COPY claude claude
RUN cargo build --release --locked

FROM quay.io/podman/stable:v5.6 AS executor
COPY --from=build /src/target/release/rubricas /usr/local/bin/rubricas
ENTRYPOINT ["rubricas"]
CMD ["executor", "--continuo"]

FROM gcr.io/distroless/cc-debian12 AS servidor
COPY --from=build /src/target/release/rubricas /usr/local/bin/rubricas
VOLUME /dados
EXPOSE 8080
ENTRYPOINT ["rubricas"]
CMD ["servir", "--dados", "/dados", "--endereco", "0.0.0.0:8080"]
