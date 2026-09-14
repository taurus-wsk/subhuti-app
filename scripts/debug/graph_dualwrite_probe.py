#!/usr/bin/env python3
"""验证 SQLite 降级模式下 EntityGraph 运行时双写是否落盘。

背景：此前 SQLite 模式下 `EntityGraph` 无持久化后端，`register_chunk` /
建边 / 反馈只写内存，`graph_entity_chunks` 恒 0（只有启动回灌、无运行时增量）。
本探针写入一条唯一事实触发沉淀，随后直接查库确认增量已落盘。
"""
import json
import os
import sqlite3
import subprocess
import threading
import time

BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")
DATA_DIR = os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/.subhuti/data"))
DB = os.path.join(DATA_DIR, "sutra_library.sqlite")

proc = subprocess.Popen([BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, text=True, bufsize=1)
_resp, _ev = {}, threading.Condition()


def _reader():
    for ln in proc.stdout:
        ln = ln.strip()
        if not ln:
            continue
        try:
            m = json.loads(ln)
        except json.JSONDecodeError:
            continue
        with _ev:
            if m.get("id") is not None:
                _resp[m["id"]] = m
            _ev.notify_all()


threading.Thread(target=_reader, daemon=True).start()
_id = 0


def req(method, params=None, timeout=180):
    global _id
    _id += 1
    rid = _id
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid,
                                 "method": method, "params": params or {}}) + "\n")
    proc.stdin.flush()
    dl = time.time() + timeout
    with _ev:
        while rid not in _resp:
            rem = dl - time.time()
            if rem <= 0:
                raise TimeoutError(method)
            _ev.wait(rem)
        return _resp.pop(rid)


def call(name, args, timeout=180):
    r = req("tools/call", {"name": name, "arguments": args}, timeout)
    res = r.get("result") or {}
    return bool(res.get("isError")), ((res.get("content") or [{}])[0].get("text", "") or "")


def counts():
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        return {t: con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
                for t in ("memory_nodes", "graph_edges",
                          "graph_entity_chunks", "graph_entity_feedback")}
    finally:
        con.close()


req("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "graph-probe", "version": "1.0"}}, timeout=30)

print("库路径:", DB)
print("before:", json.dumps(counts(), ensure_ascii=False))

uniq = "graphprobe" + str(int(time.time()))
content = f"{uniq} 记录：Tokio 运行时依赖 Futures 抽象，glTF 导出依赖 KHR 材质扩展"
err, txt = call("subhuti_memory", {"action": "write", "content": content,
                                   "domain": "general", "session_id": uniq})
print("write isError:", err, "->", txt[:200])

# register_chunk / 建边的双写是 tokio::spawn 异步执行，等它落盘
time.sleep(3)
after = counts()
print("after :", json.dumps(after, ensure_ascii=False))

proc.terminate()
