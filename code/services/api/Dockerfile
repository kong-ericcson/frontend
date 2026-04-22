FROM rust:bookworm AS builder
WORKDIR /app

# 1. Copy only manifests and create dummy source files to cache dependencies
COPY code/bluetext-model/Cargo.toml ./code/bluetext-model/Cargo.toml
COPY code/bluetext-model-core/Cargo.toml ./code/bluetext-model-core/Cargo.toml
COPY code/bluetext-model-macros/Cargo.toml ./code/bluetext-model-macros/Cargo.toml
COPY code/clients/Cargo.toml ./code/clients/Cargo.toml
COPY model/Cargo.toml ./model/Cargo.toml
COPY code/services/api/Cargo.toml ./code/services/api/Cargo.toml

RUN mkdir -p code/bluetext-model/src && echo "" > code/bluetext-model/src/lib.rs \
    && mkdir -p code/bluetext-model-core/src && echo "" > code/bluetext-model-core/src/lib.rs \
    && mkdir -p code/bluetext-model-macros/src && echo "fn main(){}" > code/bluetext-model-macros/src/lib.rs \
    && mkdir -p code/clients/src && echo "" > code/clients/src/lib.rs \
    && mkdir -p model/src && echo "fn main(){}" > model/src/main.rs && echo "" > model/src/lib.rs \
    && mkdir -p code/services/api/src && echo "fn main(){}" > code/services/api/src/main.rs

# 2. Build dependencies only (cached layer — survives source changes)
WORKDIR /app/code/services/api
RUN cargo build --release 2>/dev/null || true
WORKDIR /app

# 3. Copy actual source code
COPY code/bluetext-model/ ./code/bluetext-model/
COPY code/bluetext-model-core/ ./code/bluetext-model-core/
COPY code/bluetext-model-macros/ ./code/bluetext-model-macros/
COPY code/clients/ ./code/clients/
COPY model/ ./model/
COPY code/services/api/ ./code/services/api/

# 4. Build the real binary (only recompiles changed crates)
WORKDIR /app/code/services/api
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/code/services/api/target/release/api /usr/local/bin/api
ENV PORT=3030
EXPOSE 3030
CMD ["api"]
