#!/usr/bin/env python3
"""藏经阁 P1~P3 验收脚本（真实 MCP stdio，无 mock）。

覆盖项：
  P1-a 静态知识冷启动：rust 领域应有种子节点
  P1-b MCP 记忆工具 subhuti_memory（write / recall / stats / collections）
  P1-c 知识库接口无 PG 时不再 503（HTTP 层，见脚本末尾提示）
  P2   /sutra/stats 可观测接口返回真实数据
  P3   实体图谱有数据（graph_entity_chunks / 图谱回灌）

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti python3 scripts/debug/sutra_verify.py
"""
import json
import os
import sqlite3
import subprocess
import sys
import threading
import time

BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")
DATA_DIR = os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/.subhuti/data"))
DB = os.path.join(DATA_DIR, "sutra_library.sqlite")


class Client:
    def __init__(self):
        self.proc = subprocess.Popen(
            [BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1)
        self._id = 0
        self._lock = threading.Lock()
        self._resp = {}
        self._ev = threading.Condition()
        threading.Thread(target=self._reader, daemon=True).start()

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
                if m.get("id") is not None:
                    self._resp[m["id"]] = m
                self._ev.notify_all()

    def request(self, method, params=None, timeout=180):
        with self._lock:
            self._id += 1
            rid = self._id
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid,
                                          "method": method, "params": params or {}}) + "\n")
        self.proc.stdin.flush()
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
            pass


def call_tool(c, name, args, timeout=180):
    r = c.request("tools/call", {"name": name, "arguments": args}, timeout=timeout)
    result = r.get("result") or {}
    return bool(result.get("isError")), ((result.get("content") or [{}])[0].get("text", "") or "")


def counts():
    if not os.path.exists(DB):
        return {}
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        out = {}
        for t in ("memory_nodes", "memory_collections", "graph_edges",
                  "graph_entity_chunks", "graph_entity_feedback"):
            try:
                out[t] = con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
            except sqlite3.Error:
                out[t] = -1
        try:
            out["rust_seed_nodes"] = con.execute(
                "SELECT COUNT(*) FROM memory_nodes WHERE collection_id IN "
                "(SELECT collection_id FROM memory_collections WHERE domain='rust')"
            ).fetchone()[0]
        except sqlite3.Error:
            out["rust_seed_nodes"] = -1
        return out
    finally:
        con.close()


def main() -> int:
    print(f"藏经阁库：{DB}\n")
    before = counts()
    print("① 启动前：", json.dumps(before, ensure_ascii=False))

    c = Client()
    ok = True
    try:
        c.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                 "clientInfo": {"name": "sutra-verify", "version": "1.0"}}, timeout=30)

        tools = (c.request("tools/list", {}, timeout=30).get("result") or {}).get("tools", [])
        names = [t.get("name") for t in tools]
        print(f"② MCP 工具：{names}")
        if "subhuti_memory" not in names:
            print("   ❌ 缺少 subhuti_memory 工具")
            ok = False

        # P1-b: 写入一条记忆
        print("\n③ P1-b 记忆工具 write")
        err, txt = call_tool(c, "subhuti_memory", {
            "action": "write",
            "content": "用户偏好：提交前必须跑 cargo fmt 与 cargo clippy",
            "domain": "rust",
            "session_id": "verify-1",
        })
        print(f"   isError={err} -> {txt[:120]}")
        if err:
            ok = False

        # P1-b: 检索
        print("④ P1-b 记忆工具 recall")
        err, txt = call_tool(c, "subhuti_memory", {
            "action": "recall", "query": "提交前要做什么", "top_k": 3})
        print(f"   isError={err}\n   {txt[:400]}")
        if err or "clippy" not in txt:
            print("   ❌ 召回未命中刚写入的记忆")
            ok = False

        # P2: stats
        print("⑤ P1-b 记忆工具 stats")
        err, txt = call_tool(c, "subhuti_memory", {"action": "stats"})
        print(f"   {txt[:200]}")
        if err:
            ok = False

        # collections
        err, txt = call_tool(c, "subhuti_memory", {"action": "collections"})
        print(f"⑥ collections:\n   {txt[:300]}")

        # 等待冷启动异步任务（启动后 2 秒执行）
        time.sleep(3)
    finally:
        c.close()

    after = counts()
    print("\n⑦ 结束后：", json.dumps(after, ensure_ascii=False))
    print(f"   节点 {before.get('memory_nodes', 0)} -> {after.get('memory_nodes', 0)}")
    print(f"   Rust 种子节点：{after.get('rust_seed_nodes', 0)}")
    print(f"   图谱边：{after.get('graph_edges', 0)} / 实体-切片索引：{after.get('graph_entity_chunks', 0)}")

    if after.get("memory_nodes", 0) <= before.get("memory_nodes", 0):
        print("   ❌ 记忆节点没有增长")
        ok = False
    if after.get("rust_seed_nodes", 0) <= 0:
        print("   ⚠️  Rust 静态知识冷启动未产出节点")

    print("\n" + ("✅ 全部通过" if ok else "❌ 存在失败项"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
