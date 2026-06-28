#!/bin/bash
# ============================================================
# Subhuti 开发环境启动脚本
# 用法: ./dev.sh [build|start|stop|restart|status|logs|test]
# ============================================================
set -e

# 项目根目录（脚本在 scripts/build/ 下，需要上两级）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BINARY="$PROJECT_DIR/target/release/http_server"
PID_FILE="$PROJECT_DIR/.http_server.pid"
LOG_DIR="$PROJECT_DIR/logs"

# 环境变量（连接本地 pgvector-db-new）
export DB_HOST="${DB_HOST:-localhost}"
export DB_PORT="${DB_PORT:-5432}"
export DB_DATABASE="${DB_DATABASE:-postgres}"
export DB_USERNAME="${DB_USERNAME:-postgres}"
export DB_PASSWORD="${DB_PASSWORD:-123456}"
export DB_MAX_CONN="${DB_MAX_CONN:-10}"
export HTTP_ADDR="${HTTP_ADDR:-0.0.0.0:8080}"
export RUST_LOG="${RUST_LOG:-info}"

# 加载 .env（API key 等）
if [ -f "$PROJECT_DIR/.env" ]; then
    set -a
    source "$PROJECT_DIR/.env"
    set +a
fi

build() {
    echo "🔨 编译 release 版本..."
    cd "$PROJECT_DIR"
    cargo build --release --bin http_server
    echo "✅ 编译完成: $BINARY"
}

start() {
    if is_running; then
        echo "⚠️  服务已在运行 (PID: $(cat "$PID_FILE"))"
        return 1
    fi

    # 确保 8080 端口可用
    if lsof -ti:8080 > /dev/null 2>&1; then
        echo "⚠️  端口 8080 被占用，正在释放..."
        lsof -ti:8080 | xargs kill -9 2>/dev/null || true
        sleep 1
    fi

    mkdir -p "$LOG_DIR"

    echo "🚀 启动 Subhuti HTTP Server..."
    echo "   地址: http://$HTTP_ADDR"
    echo "   数据库: $DB_HOST:$DB_PORT/$DB_DATABASE"
    echo "   日志: $LOG_DIR/"

    nohup "$BINARY" >> "$LOG_DIR/subhuti.log" 2>&1 &
    echo $! > "$PID_FILE"

    # 等待启动
    sleep 2
    if is_running; then
        echo "✅ 启动成功 (PID: $(cat "$PID_FILE"))"
        echo "   测试页面: http://localhost:8080/subhuti/test/index.html"
        echo "   健康检查: http://localhost:8080/subhuti/api/v1/health"
    else
        echo "❌ 启动失败，查看日志: $LOG_DIR/subhuti.log"
        return 1
    fi
}

stop() {
    if ! is_running; then
        echo "ℹ️  服务未运行"
        return 0
    fi

    local pid=$(cat "$PID_FILE")
    echo "🛑 停止服务 (PID: $pid)..."
    kill "$pid" 2>/dev/null || true
    sleep 1
    kill -9 "$pid" 2>/dev/null || true
    rm -f "$PID_FILE"
    echo "✅ 已停止"
}

restart() {
    stop
    sleep 1
    start
}

status() {
    if is_running; then
        local pid=$(cat "$PID_FILE")
        echo "✅ 运行中 (PID: $pid)"
        echo "   地址: http://$HTTP_ADDR"
        curl -sf "http://localhost:8080/subhuti/api/v1/health" 2>/dev/null && echo "" || echo "   ⚠️ 健康检查失败"
    else
        echo "🔴 未运行"
    fi
}

logs() {
    if [ -f "$LOG_DIR/subhuti.log" ]; then
        tail -f "$LOG_DIR/subhuti.log"
    else
        echo "ℹ️  暂无日志"
    fi
}

test_health() {
    echo "🔍 健康检查..."
    curl -sf "http://localhost:8080/subhuti/api/v1/health" && echo "" || echo "❌ 服务不可达"
    echo ""
    echo "🔍 详细状态..."
    curl -sf "http://localhost:8080/subhuti/api/v1/health/detailed" | python3 -m json.tool 2>/dev/null || echo "❌ 详细状态不可达"
}

is_running() {
    [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

case "${1:-start}" in
    build)   build ;;
    start)   start ;;
    stop)    stop ;;
    restart) restart ;;
    status)  status ;;
    logs)    logs ;;
    test)    test_health ;;
    *)
        echo "用法: ./dev.sh [build|start|stop|restart|status|logs|test]"
        exit 1
        ;;
esac
