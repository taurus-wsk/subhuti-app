#!/usr/bin/env bash

set -e

LOG_DIR="./logs"

find_log_file() {
    latest=$(ls -1 "$LOG_DIR/subhuti.log".* 2>/dev/null | sort -r | head -1)
    if [ -n "$latest" ]; then
        echo "$latest"
        return
    fi
    if [ -f "$LOG_DIR/subhuti.log" ]; then
        echo "$LOG_DIR/subhuti.log"
        return
    fi
    echo ""
}

case "$1" in
    trace)
        if [ -z "$2" ]; then
            echo "用法: $0 trace <trace_id>"
            exit 1
        fi
        LOG_FILE=$(find_log_file)
        if [ -z "$LOG_FILE" ]; then
            echo "错误: 未找到日志文件"
            exit 1
        fi
        echo "📋 查询 trace_id: $2"
        echo "─────────────────────────────────────────────────────────────"
        grep "$2" "$LOG_FILE" | jq "select(.span != null)" | \
            jq '{timestamp, level, span: .span.name, chain: .span.chain, message: .fields.message, duration: .fields["time.busy"]}'
        ;;

    expert)
        if [ -z "$2" ]; then
            echo "用法: $0 expert <expert_id>"
            exit 1
        fi
        LOG_FILE=$(find_log_file)
        if [ -z "$LOG_FILE" ]; then
            echo "错误: 未找到日志文件"
            exit 1
        fi
        echo "📋 查询 expert_id: $2"
        echo "─────────────────────────────────────────────────────────────"
        grep "$2" "$LOG_FILE" | jq "select(.span != null)" | \
            jq '{timestamp, level, span: .span.name, message: .fields.message, duration: .fields["time.busy"]}'
        ;;

    user)
        if [ -z "$2" ]; then
            echo "用法: $0 user <user_id>"
            exit 1
        fi
        LOG_FILE=$(find_log_file)
        if [ -z "$LOG_FILE" ]; then
            echo "错误: 未找到日志文件"
            exit 1
        fi
        echo "📋 查询 user_id: $2"
        echo "─────────────────────────────────────────────────────────────"
        grep "$2" "$LOG_FILE" | jq "select(.span != null)" | \
            jq '{timestamp, level, span: .span.name, message: .fields.message}'
        ;;

    session)
        if [ -z "$2" ]; then
            echo "用法: $0 session <session_id>"
            exit 1
        fi
        LOG_FILE=$(find_log_file)
        if [ -z "$LOG_FILE" ]; then
            echo "错误: 未找到日志文件"
            exit 1
        fi
        echo "📋 查询 session_id: $2"
        echo "─────────────────────────────────────────────────────────────"
        grep "$2" "$LOG_FILE" | jq "select(.span != null)" | \
            jq '{timestamp, level, span: .span.name, message: .fields.message}'
        ;;

    recent)
        COUNT="${2:-20}"
        LOG_FILE=$(find_log_file)
        if [ -z "$LOG_FILE" ]; then
            echo "错误: 未找到日志文件"
            exit 1
        fi
        echo "📋 最近 $COUNT 条日志"
        echo "─────────────────────────────────────────────────────────────"
        tail -"$COUNT" "$LOG_FILE" | jq "select(.span != null)" | \
            jq '{timestamp, level, span: .span.name, message: .fields.message}'
        ;;

    errors)
        LOG_FILE=$(find_log_file)
        if [ -z "$LOG_FILE" ]; then
            echo "错误: 未找到日志文件"
            exit 1
        fi
        echo "📋 错误日志"
        echo "─────────────────────────────────────────────────────────────"
        grep '"level":"ERROR"' "$LOG_FILE" | jq '{timestamp, message: .fields.message}'
        ;;

    list)
        echo "📂 日志文件列表:"
        echo "─────────────────────────────────────────────────────────────"
        ls -la "$LOG_DIR/subhuti.log"* 2>/dev/null || echo "无日志文件"
        ;;

    help|*)
        echo "Subhuti 日志查询工具"
        echo ""
        echo "用法:"
        echo "  $0 trace <trace_id>      按 trace_id 查询"
        echo "  $0 expert <expert_id>    按专家 ID 查询"
        echo "  $0 user <user_id>        按用户 ID 查询"
        echo "  $0 session <session_id>  按会话 ID 查询"
        echo "  $0 recent [条数]         查看最近日志"
        echo "  $0 errors                查看错误日志"
        echo "  $0 list                  列出日志文件"
        echo ""
        echo "示例:"
        echo "  $0 trace a94f42f6-e3d6-4504-a824-45452970ef4e"
        echo "  $0 recent 10"
        ;;
esac