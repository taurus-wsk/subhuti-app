#!/usr/bin/env python3
"""最小 MCP 客户端 harness：驱动 subhuti mcp (stdio JSON-RPC)，抓取协议响应与日志。

用法: python3 scripts/mcp_debug_client.py
"""
import json
import subprocess
import sys
import time
import threading
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target/debug/subhuti")


def req(rid, method, params=None):
    m = {"jsonrpc": "2.0", "id": rid, "method": method}
    if params is not None:
        m["params"] = params
    return m


def notif(method, params=None):
    m = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        m["params"] = params
    return m


def main():
    env = dict(os.environ)
    env.setdefault("RUST_BACKTRACE", "1")

    proc = subprocess.Popen(
        [BIN, "mcp"],
        cwd=ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        text=True,
        bufsize=1,
    )

    ready = threading.Event()

    responses = []

    def read_stdout():
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            responses.append(line)
            print(f"[STDOUT] {line[:300]}")

    def read_stderr():
        for line in proc.stderr:
            if "已就绪" in line:
                ready.set()
            print(f"[STDERR] {line.rstrip()[:300]}")

    t_out = threading.Thread(target=read_stdout, daemon=True)
    t_err = threading.Thread(target=read_stderr, daemon=True)
    t_out.start()
    t_err.start()

    def send(m):
        proc.stdin.write(json.dumps(m) + "\n")
        proc.stdin.flush()

    # MCP 启动较慢（PG 探测超时 5s + LLM 健康检查），必须等就绪再发请求
    print("等待 MCP 就绪 ...")
    if not ready.wait(timeout=30):
        print("❌ 30s 内未就绪")
    else:
        print("✅ MCP 已就绪，开始发请求")

    send(req(1, "initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "debug-client", "version": "0.1"},
    }))
    time.sleep(1.0)
    send(notif("notifications/initialized"))
    time.sleep(0.3)

    send(req(2, "tools/list"))
    time.sleep(1.0)

    send(req(3, "tools/call", {"name": "subhuti_list_experts", "arguments": {}}))
    time.sleep(1.5)

    expected = 3

    # --chat：额外跑一次真实编排（需要 LLM），用于验证 trace / span / fn_call 落库
    if "--chat" in sys.argv:
        send(req(4, "tools/call", {
            "name": "subhuti_chat",
            "arguments": {"message": "用一句话解释 Rust 的所有权是什么。"},
        }))
        expected = 4

    # 等到收齐预期响应或超时
    deadline = time.time() + (300 if expected == 4 else 15)
    while len(responses) < expected and time.time() < deadline:
        time.sleep(0.3)

    print("\n===== 收到响应数: %d =====" % len(responses))
    for line in responses:
        try:
            o = json.loads(line)
        except Exception:
            print("  RAW:", line[:200])
            continue
        if "method" in o:
            print("  NOTIF:", o.get("method"))
            continue
        r = o.get("result", {})
        e = o.get("error")
        if e:
            print(f"  id={o.get('id')} ERROR: {json.dumps(e, ensure_ascii=False)[:300]}")
        elif "protocolVersion" in r:
            print(f"  id={o.get('id')} initialize -> {r.get('protocolVersion')} server={r.get('serverInfo')}")
        elif "tools" in r:
            print(f"  id={o.get('id')} tools/list -> {[t['name'] for t in r['tools']]}")
        elif "content" in r:
            txt = r["content"][0].get("text", "") if r["content"] else ""
            print(f"  id={o.get('id')} tools/call -> {txt[:400]}")
        else:
            print(f"  id={o.get('id')} other: {json.dumps(o, ensure_ascii=False)[:200]}")

    print("\n===== 关闭 stdin，等待进程退出 =====")
    proc.stdin.close()
    try:
        rc = proc.wait(timeout=10)
        print("EXIT CODE =", rc)
    except subprocess.TimeoutExpired:
        print("进程未在 10s 内退出，kill")
        proc.kill()
    time.sleep(0.5)


if __name__ == "__main__":
    main()
