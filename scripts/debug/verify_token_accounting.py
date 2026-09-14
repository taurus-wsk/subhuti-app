#!/usr/bin/env python3
"""验证 token 成本核算真的落库（真实 stdio JSON-RPC，无 mock）。

背景：`span.tokens` 只由框架的 `LLMResponded.tokens_used` 填充，而生产代码此前
从不发这个事件 → `total_tokens` 恒为 0。本脚本走一次真实领域内请求，然后直接查
链路库确认「带 token 的 span」确实新增且数值 > 0。

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti python3 scripts/debug/verify_token_accounting.py

环境变量：
    SUBHUTI_BIN          被测二进制，默认 target/debug/subhuti
    SUBHUTI_TRACE_SQLITE 链路库路径；缺省时由 SUBHUTI_DATA_DIR 推导
"""
import json
import os
import sqlite3
import subprocess
import sys
import threading
import time

BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")


def trace_db_path() -> str:
    p = os.environ.get("SUBHUTI_TRACE_SQLITE")
    if p:
        return p
    data_dir = os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/.subhuti/data"))
    return os.path.join(data_dir, "traces.sqlite")


def snapshot(path: str) -> dict:
    """(带 token 的 span 数, token 总和)。库/表不存在时返回全 0。"""
    if not os.path.exists(path):
        return {"count": 0, "sum": 0, "spans": 0}
    c = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        spans = c.execute("SELECT COUNT(*) FROM trace_spans").fetchone()[0]
        row = c.execute(
            "SELECT COUNT(*), COALESCE(SUM(tokens),0) FROM trace_spans WHERE tokens IS NOT NULL"
        ).fetchone()
        return {"count": row[0], "sum": int(row[1]), "spans": spans}
    except sqlite3.Error:
        return {"count": 0, "sum": 0, "spans": 0}
    finally:
        c.close()


def recent_token_spans(path: str, limit: int = 6) -> list:
    c = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        rows = c.execute(
            "SELECT name, tokens, duration_ms FROM trace_spans "
            "WHERE tokens IS NOT NULL AND tokens > 0 "
            "ORDER BY rowid DESC LIMIT ?",
            (limit,),
        ).fetchall()
        return rows
    except sqlite3.Error:
        return []
    finally:
        c.close()


class Client:
    """最小 MCP 客户端：后台线程读，按 id 分发。"""

    def __init__(self):
        self.proc = subprocess.Popen(
            [BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1)
        self._id = 0
        self._lock = threading.Lock()
        self._resp = {}
        self._ev = threading.Condition()
        self._stderr = []
        threading.Thread(target=self._reader, daemon=True).start()
        threading.Thread(target=self._drain_stderr, daemon=True).start()

    def _drain_stderr(self):
        for ln in self.proc.stderr:
            self._stderr.append(ln.rstrip())
            if len(self._stderr) > 100:
                self._stderr.pop(0)

    def _reader(self):
        for ln in self.proc.stdout:
            ln = ln.strip()
            if not ln:
                continue
            try:
                m = json.loads(ln)
            except json.JSONDecodeError:
                continue
            with self._ev:
                if "id" in m and m["id"] is not None:
                    self._resp[m["id"]] = m
                self._ev.notify_all()

    def _send(self, o):
        self.proc.stdin.write(json.dumps(o) + "\n")
        self.proc.stdin.flush()

    def request(self, method, params=None, timeout=400):
        with self._lock:
            self._id += 1
            rid = self._id
        self._send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}})
        dl = time.time() + timeout
        with self._ev:
            while rid not in self._resp:
                rem = dl - time.time()
                if rem <= 0:
                    raise TimeoutError(f"id={rid} method={method}")
                self._ev.wait(rem)
            return self._resp.pop(rid)

    def close(self):
        try:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        except Exception:
            try:
                self.proc.kill()
            except Exception:
                pass


def main() -> int:
    db = trace_db_path()
    print(f"链路库：{db}")
    print(f"被测二进制：{BIN}")
    if not os.path.exists(db):
        print(f"❌ 链路库不存在，先跑一次服务让它建库：{db}")
        return 2

    before = snapshot(db)
    print(f"\n请求前：span 总数={before['spans']}  带 token 的 span={before['count']}  token 总量={before['sum']}")

    c = Client()
    try:
        c.request("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "token-verify", "version": "1.0"},
        }, timeout=30)

        t0 = time.time()
        resp = c.request("tools/call", {
            "name": "subhuti_chat",
            "arguments": {
                "message": "用一句话说明 Rust 的所有权机制",
                "session_id": "token-verify",
            },
        }, timeout=300)
        dt = time.time() - t0
        is_err = bool((resp.get("result") or {}).get("isError"))
        print(f"\n请求返回：{dt:.1f}s  isError={is_err}")
        if is_err:
            txt = ((resp.get("result") or {}).get("content") or [{}])[0].get("text", "")
            print(f"  ⚠️ 工具报错：{txt[:200]}")
    finally:
        c.close()

    # 链路是异步落盘的，给一点时间再查
    after = before
    for _ in range(20):
        time.sleep(0.5)
        after = snapshot(db)
        if after["count"] > before["count"]:
            break

    print(f"请求后：span 总数={after['spans']}  带 token 的 span={after['count']}  token 总量={after['sum']}")
    print(f"增量：span +{after['spans'] - before['spans']}  "
          f"带 token 的 span +{after['count'] - before['count']}  "
          f"token +{after['sum'] - before['sum']}")

    samples = recent_token_spans(db)
    if samples:
        print("\n最近带用量的 span 样本：")
        for name, tokens, dur in samples:
            print(f"  {name:<20} tokens={tokens:<8} duration_ms={dur}")

    ok = after["count"] > before["count"] and after["sum"] > before["sum"]
    print("\n" + ("✅ 通过：token 成本已真实落库" if ok
                  else "❌ 失败：本次请求没有产生带 token 的 span"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
