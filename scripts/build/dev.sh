#!/bin/bash
# ============================================================
# Subhuti 开发环境启动脚本
# 用法: ./dev.sh [build|start|stop|restart|status|logs|test] [debug|release]
# ============================================================
set -e

# 项目根目录（脚本在 scripts/build/ 下，需要上两级）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_MODE="${2:-release}"

if [ "$BUILD_MODE" = "debug" ]; then
    BINARY="$PROJECT_DIR/target/debug/subhuti"
    RUST_LOG_DEFAULT="debug,tower_http=off,hyper=off,reqwest=off,sqlx=off"
else
    BINARY="$PROJECT_DIR/target/release/subhuti"
    RUST_LOG_DEFAULT="info,tower_http=off,hyper=off,reqwest=off,sqlx=off"
fi

PID_FILE="$PROJECT_DIR/.http_server.pid"
LOG_DIR="$PROJECT_DIR/logs"

# 所有业务配置以 config/Subhuti.toml 为准，脚本不再 export 覆盖
# RUST_LOG 是 tracing 标准环境变量，不由 Subhuti.toml 管理
export RUST_LOG="${RUST_LOG:-$RUST_LOG_DEFAULT}"

# 加载 .env（API key、SUBHUTI_LLM_CACHE 等本地配置）
# 注意：set -a 让所有变量自动 export，子进程（subhuti 二进制）能读到
if [ -f "$PROJECT_DIR/.env" ]; then
    set -a
    # shellcheck disable=SC1090
    source "$PROJECT_DIR/.env"
    set +a
fi

build() {
    echo "🔨 编译 release 版本..."
    cd "$PROJECT_DIR"
    cargo build --release --bin subhuti
    echo "✅ 编译完成: $BINARY"
}

# 关闭所有 subhuti serve 进程（含孤儿/多端口实例），再启动单一 8615 服务，
# 避免多个实例并存导致日志重复、端口混淆。
stop_all() {
    echo "🛑 关闭所有 Subhuti serve 进程..."

    # 释放端口占用（8615 及可能残留的 8080 等）
    for port in 8615 8080; do
        if lsof -ti:"$port" > /dev/null 2>&1; then
            echo "  释放端口 $port ..."
            lsof -ti:"$port" | xargs kill -9 2>/dev/null || true
        fi
    done

    # 兜底：按项目目标二进制路径匹配，杀掉残余的 subhuti serve 进程
    pgrep -f "$PROJECT_DIR/target/.*/subhuti serve" | xargs kill -9 2>/dev/null || true

    rm -f "$PID_FILE"
    sleep 1
}

start() {
    # 先关闭所有已存在的 subhuti serve 进程，保证只留一个新起的 8615 服务
    stop_all

    if [ "$BUILD_MODE" = "debug" ]; then
        echo "🔨 编译 debug 版本..."
        cd "$PROJECT_DIR"
        cargo build --bin subhuti
    fi

    mkdir -p "$LOG_DIR"

    echo "🚀 启动 Subhuti HTTP Server..."
    echo "   配置: config/Subhuti.toml"
    echo "   日志: $LOG_DIR/"

    # Mock 模式默认关闭（之前 debug 默认带 --mock 会强制走 MockLLM）
    # 想用 mock 调试：SUBHUTI_MOCK=1 make serve-debug
    # 本地默认走真实 LLM + .env 里的 SUBHUTI_LLM_CACHE=1 自动生效
    if [ "$SUBHUTI_MOCK" = "1" ]; then
        echo "   🧪 Mock 模式: config/subhuti.md"
        nohup "$BINARY" serve --mock --mock-file config/subhuti.md >> "$LOG_DIR/subhuti.log" 2>&1 &
    else
        if [ "$SUBHUTI_LLM_CACHE" = "1" ]; then
            echo "   🧪 LLM 调试缓存: 已开启 (.llm-cache.json)"
        else
            echo "   🚫 Mock: 已禁用（走真实 LLM）"
        fi
        nohup "$BINARY" serve >> "$LOG_DIR/subhuti.log" 2>&1 &
    fi
    echo $! > "$PID_FILE"

    # 等待启动
    sleep 2
    if is_running; then
        echo "✅ 启动成功 (PID: $(cat "$PID_FILE"))"
        echo "   健康检查: http://localhost:8615/subhuti/api/v1/health"
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

start_log() {
    # 先启动服务，然后持续 tail 日志
    start
    echo ""
    echo "📋 持续查看日志 (Ctrl+C 退出，服务继续运行)..."
    echo "─────────────────────────────────────────────────────────────"
    logs
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
        curl -sf "http://localhost:8615/subhuti/api/v1/health" 2>/dev/null && echo "" || echo "   ⚠️ 健康检查失败"
    else
        echo "🔴 未运行"
    fi
}

# 清理遗留的 tail 进程：避免多个 tail 同时跟随同一日志文件，
# 导致同一条新日志被重复打印（"日志重复"的根因）。
kill_old_tails() {
    pgrep -f "tail.*$LOG_DIR/subhuti\.log" | xargs kill -9 2>/dev/null || true
    sleep 0.2
}

logs() {
    # 先清理旧的 tail，保证只有一个 tail 在跟随日志文件
    kill_old_tails
    if [ -f "$LOG_DIR/subhuti.log" ]; then
        # -n 0：不回溯历史行，只显示本次启动后的新日志，避免把历次启动遗留的
        # "Server listening" 等旧日志一起打印出来，造成"日志重复"的错觉。
        tail -n 0 -f "$LOG_DIR/subhuti.log"
    else
        echo "ℹ️  暂无日志"
    fi
}

test_health() {
    echo "🔍 健康检查..."
    curl -sf "http://localhost:8615/subhuti/api/v1/health" && echo "" || echo "❌ 服务不可达"
    echo ""
    echo "🔍 详细状态..."
    curl -sf "http://localhost:8615/subhuti/api/v1/health/detailed" | python3 -m json.tool 2>/dev/null || echo "❌ 详细状态不可达"
}

is_running() {
    [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

case "${1:-start}" in
    build)      build ;;
    start)      start ;;
    start-log)  start_log ;;
    stop)       stop ;;
    stop-all)   stop_all ;;
    restart)    restart ;;
    status)     status ;;
    logs)       logs ;;
    test)       test_health ;;
    *)
        echo "用法: ./dev.sh [build|start|start-log|stop|stop-all|restart|status|logs|test]"
        echo ""
        echo "  start       启动前先关闭所有 subhuti serve，再后台启动单一 8615 服务（默认）"
        echo "  start-log   同 start，随后持续查看日志（Ctrl+C 退出，服务继续运行）"
        echo "  stop        停止服务"
        echo "  stop-all    关闭所有 subhuti serve 进程（含孤儿/多端口实例）"
        echo "  logs        查看已有服务的日志"
        exit 1
        ;;
esac
