#!/bin/bash
# ============================================================
# Subhuti Docker 管理脚本
# 用法: ./docker.sh [build|start|stop|restart|status|logs]
# ============================================================
set -e

IMAGE_NAME="subhuti"
CONTAINER_NAME="subhuti-app"
PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"

build() {
    echo "🔨 构建 Docker 镜像..."
    cd "$PROJECT_DIR"
    docker build -t "$IMAGE_NAME" .
    echo "✅ 镜像构建完成"
}

start() {
    # 先停止已有容器
    if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        echo "🛑 移除旧容器..."
        docker rm -f "$CONTAINER_NAME" 2>/dev/null || true
    fi

    # 确保宿主机端口 8080 可用
    if lsof -ti:8080 > /dev/null 2>&1; then
        echo "⚠️  端口 8080 被占用，正在释放..."
        lsof -ti:8080 | xargs kill -9 2>/dev/null || true
        sleep 1
    fi

    echo "🚀 启动 Docker 容器..."
    docker run -d \
        --name "$CONTAINER_NAME" \
        -p 8080:8080 \
        --env-file "$PROJECT_DIR/.env" \
        -e DB_HOST=host.docker.internal \
        -e DB_PORT=5432 \
        -e DB_DATABASE=postgres \
        -e DB_USERNAME=postgres \
        -e DB_PASSWORD=123456 \
        -e DB_MAX_CONN=10 \
        -e HTTP_ADDR=0.0.0.0:8080 \
        -e RUST_LOG=info \
        -v "$PROJECT_DIR/logs:/app/logs" \
        "$IMAGE_NAME"

    echo "✅ 容器已启动"
    echo "   测试页面: http://localhost:8080/subhuti/test/index.html"
    echo "   健康检查: http://localhost:8080/subhuti/api/v1/health"
}

stop() {
    echo "🛑 停止容器..."
    docker stop "$CONTAINER_NAME" 2>/dev/null || true
    docker rm "$CONTAINER_NAME" 2>/dev/null || true
    echo "✅ 容器已停止并移除"
}

restart() {
    stop
    start
}

status() {
    if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        echo "✅ 运行中"
        docker ps --filter "name=$CONTAINER_NAME" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
    elif docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        echo "🔴 已停止"
        docker ps -a --filter "name=$CONTAINER_NAME" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
    else
        echo "ℹ️  容器不存在"
    fi
}

logs() {
    docker logs -f "$CONTAINER_NAME" 2>&1
}

case "${1:-start}" in
    build)   build ;;
    start)   start ;;
    stop)    stop ;;
    restart) restart ;;
    status)  status ;;
    logs)    logs ;;
    *)
        echo "用法: ./docker.sh [build|start|stop|restart|status|logs]"
        exit 1
        ;;
esac
