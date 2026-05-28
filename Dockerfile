#### Python ####
FROM python:3.13 AS archipelago

WORKDIR /app

# python image has too old git version for this:
# RUN git clone --depth 1 --revision <rev> https://github.com/RubixDev/Archipelago.git .
# so we do it the long way
RUN git init && \
  git remote add origin https://github.com/RubixDev/Archipelago.git && \
  git fetch --depth 1 origin 9eda4d590625c04d0cb2a1ae5e4c6449bc50014c && \
  git checkout FETCH_HEAD

RUN python ModuleUpdate.py --yes

#### Rust ####

FROM lukemathwalker/cargo-chef:latest-rust-latest AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.* .
COPY src/ ./src/
RUN cargo build --release

FROM archipelago
COPY --from=builder /app/target/release/ap-index /app/ap-index
COPY index.toml .
ENTRYPOINT [ "/app/ap-index" ]
