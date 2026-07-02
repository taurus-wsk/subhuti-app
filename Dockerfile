# ============================================================
# Subhuti Docker 镜像 - 多阶段构建
# ============================================================
# 构建: docker compose up --build -d
# ============================================================

# ============ Stage 1: Build ============
FROM rust:bookworm AS builder

WORKDIR /app

# 安装构建依赖
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 先复制依赖清单，利用 Docker 层缓存加速重复构建
COPY Cargo.toml Cargo.lock ./
COPY crates/subhuti/Cargo.toml crates/subhuti/
COPY crates/subhuti-expert-psychology/Cargo.toml crates/subhuti-expert-psychology/

# 创建占位源码，让依赖预编译通过（利用缓存层）
RUN mkdir -p src/bin/http_server src/bin/cli \
    && echo "fn main() {}" > src/bin/http_server/main.rs \
    && echo "fn main() {}" > src/bin/cli/main.rs \
    && echo "fn main() {}" > src/bin/sync_test.rs \
    && echo "fn main() {}" > src/main.rs \
    && mkdir -p crates/subhuti/src && echo "" > crates/subhuti/src/lib.rs \
    && mkdir -p crates/subhuti-expert-psychology/src && echo "" > crates/subhuti-expert-psychology/src/lib.rs

# 预编译依赖（仅在 Cargo.toml 变更时重新执行）
RUN cargo build --release --bin http_server 2>/dev/null || true

# 复制真实源码并触发增量编译
COPY . .
RUN touch crates/subhuti/src/lib.rs \
    crates/subhuti-expert-psychology/src/lib.rs \
    src/bin/http_server/main.rs \
    && cargo build --release --bin http_server

# ============ Stage 2: Runtime ============
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    curl \
    tzdata \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/http_server /app/http_server

COPY static/ /app/static/
COPY data/ /app/data/
COPY config/ /app/config/
COPY crates/subhuti/data/ /app/crates/subhuti/data/

RUN mkdir -p /app/logs /app/trace_data

ENV DB_HOST=postgres \
    DB_PORT=5432 \
    DB_DATABASE=postgres \
    DB_USERNAME=postgres \
    DB_PASSWORD=123456 \
    DB_MAX_CONN=10 \
    HTTP_ADDR=0.0.0.0:8080 \
    RUST_LOG=info \
    TZ=Asia/Shanghai

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8080/subhuti/api/v1/health || exit 1

CMD ["/app/http_server"]
