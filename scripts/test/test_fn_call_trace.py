#!/usr/bin/env python3
"""
测试函数调用链路追踪功能：
1. 发起 orchestrate 请求
2. 访问 last/fn_call_report 获取最后一次请求的报告
"""

import requests
import webbrowser
import sys

BASE_URL = "http://127.0.0.1:8615"


def test_orchestrate():
    """发起 orchestrate 请求"""
    url = f"{BASE_URL}/subhuti/api/v1/orchestrate"
    payload = {
        "message": "你好",
        "user_id": "test-user",
        "session_id": "6ecd9f10-661f-4d08-b026-bb414c57af05"
    }

    print(f"➡️  发起请求: {url}")
    print(f"payload: {payload}")
    print()

    try:
        response = requests.post(url, json=payload, timeout=300)
        print(f"✅ 响应状态码: {response.status_code}")
        print()

        if response.status_code == 200:
            data = response.json()
            print(f"📄 响应内容:\n{data}")
            print()
            return True
        else:
            print(f"❌ 请求失败: {response.text}")
            return False
    except Exception as e:
        print(f"❌ 请求异常: {e}")
        print("请确保服务已启动: cargo run")
        return False


def open_last_fn_call_report():
    """打开最后一次 trace 的函数调用报告"""
    url = f"{BASE_URL}/subhuti/api/v1/traces/last/fn_call_report"
    print(f"➡️  打开函数调用报告: {url}")
    print()

    try:
        response = requests.get(url, timeout=10)
        print(f"✅ 响应状态码: {response.status_code}")

        if response.status_code == 200:
            # 在浏览器中打开
            webbrowser.open(url)
            print(f"🌐 已在浏览器中打开: {url}")
            return True
        else:
            print(f"❌ 请求失败: {response.text}")
            return False
    except Exception as e:
        print(f"❌ 请求异常: {e}")
        return False


def main():
    print("=" * 60)
    print("Subhuti 函数调用链路追踪测试")
    print("=" * 60)
    print()

    # 1. 测试 orchestrate 请求
    if not test_orchestrate():
        sys.exit(1)

    print("-" * 60)
    print()

    # 2. 打开函数调用报告
    if not open_last_fn_call_report():
        sys.exit(1)

    print()
    print("🎉 测试完成！请在浏览器中查看函数调用链路报告。")
    print()
    print("报告包含:")
    print("  - 函数调用树")
    print("  - 每个函数的输入/输出数据")
    print("  - 执行耗时")
    print("  - 内存变化")
    print("  - 函数执行日志")


if __name__ == "__main__":
    main()
