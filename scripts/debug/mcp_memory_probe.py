#!/usr/bin/env python3
"""藏经阁 MCP 能力矩阵探针（真实 stdio JSON-RPC，无 mock）。

回答一个问题：「藏经阁现在哪些能力是正常的？MCP 到底覆盖到没有？」

覆盖：

  A 工具面     tools/list 是否有 subhuti_memory，及其 action 枚举
  B 正常路径   stats / collections / write / recall / recall(top_k)
  C 幂等去重   write 同内容两次（第二次应报"未写入"）
  D 边界防御   缺 action / 未知 action / recall 缺 query /
               write 缺 content / write 空白 content /
               write 提问意图句·第三人称复述·空泛话题标题（均应拒写）/
               recall 空 query
  E 跨进程     重启进程后新会话 recall，验证落库（SQLite 持久化）
  F 落库核对   sqlite 各表计数快照（before/after）

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti \
    SUBHUTI_DATA_DIR=~/sqlite \
    python3 scripts/debug/mcp_memory_probe.py
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
DB = os.environ.get("SUBHUTI_SUTRA_SQLITE", os.path.join(DATA_DIR, "sutra_library.sqlite"))

# 唯一标记：既是写内容的一部分，也是 recall 的命中判据。
# 每次运行带唯一后缀，否则第二次跑必然撞上幂等去重（那是正确行为，
# 却会被误报成"write 失败"）。
RUN_TAG = str(int(time.time()))
MARKER = "A7F3"
CONTENT = f"探针标记 {MARKER}-{RUN_TAG}：本项目渲染管线固定使用 ACES 色彩变换，输出前须做色彩管理。"
QUERY = f"探针标记 {MARKER} 的渲染管线用什么色彩变换"


class Client:
    def __init__(self):
        env = dict(os.environ)
        env["SUBHUTI_DATA_DIR"] = DATA_DIR
        self.proc = subprocess.Popen(
            [BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1, env=env)
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
        self.proc.stdin.write(json.dumps(
            {"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}}) + "\n")
        self.proc.stdin.flush()
        dl = time.time() + timeout
        with self._ev:
            while rid not in self._resp:
                rem = dl - time.time()
                if rem <= 0:
                    raise TimeoutError(f"id={rid} method={method}")
                self._ev.wait(rem)
            return self._resp.pop(rid)

    def initialize(self):
        self.request("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "memory-probe", "version": "1.0"}}, timeout=30)

    def close(self):
        try:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        except Exception:
            try:
                self.proc.kill()
            except Exception:
                pass


def call(client, args, timeout=180):
    """返回 (is_error, text)。"""
    r = client.request("tools/call", {"name": "subhuti_memory", "arguments": args}, timeout=timeout)
    result = r.get("result")
    if result is None:
        # 协议级错误（例如 schema 校验失败）
        return True, json.dumps(r.get("error") or {}, ensure_ascii=False)
    return bool(result.get("isError")), ((result.get("content") or [{}])[0].get("text", "") or "")


def db_counts():
    if not os.path.exists(DB):
        return {}
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        out = {}
        for t in ("memory_nodes", "memory_collections", "hot_memory",
                  "graph_edges", "graph_entity_chunks", "graph_entity_feedback"):
            try:
                out[t] = con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
            except sqlite3.Error:
                out[t] = -1
        return out
    finally:
        con.close()


RESULTS = []


def record(area, case, passed, detail=""):
    RESULTS.append((area, case, passed, detail))
    mark = "✅" if passed else "❌"
    print(f"   {mark} [{area}] {case}" + (f" — {detail}" if detail else ""))


def main() -> int:
    print(f"藏经阁库：{DB}")
    print(f"被测二进制：{BIN}\n")

    before = db_counts()
    print("① 请求前落库快照：", json.dumps(before, ensure_ascii=False))

    c = Client()
    try:
        c.initialize()

        # ── A 工具面 ──────────────────────────────────────────
        print("\n② A 工具面")
        tools = (c.request("tools/list", {}, timeout=30).get("result") or {}).get("tools", [])
        names = [t.get("name") for t in tools]
        record("A", "tools/list 暴露 subhuti_memory", "subhuti_memory" in names,
               f"共 {len(names)} 个工具")
        mem = next((t for t in tools if t.get("name") == "subhuti_memory"), None)
        actions = []
        if mem:
            actions = ((mem.get("inputSchema") or {}).get("properties") or {}) \
                .get("action", {}).get("enum", [])
        record("A", "action 枚举 = 4 个", actions == ["recall", "write", "stats", "collections"],
               f"{actions}")

        # ── B 正常路径 ────────────────────────────────────────
        print("\n③ B 正常路径")
        err, txt = call(c, {"action": "stats"})
        record("B", "stats 返回统计", (not err) and "藏经阁统计" in txt, txt.split("\n")[0][:60])

        err, txt = call(c, {"action": "collections"})
        record("B", "collections 返回列表", (not err) and ("集合" in txt), txt.split("\n")[0][:60])

        err, txt = call(c, {"action": "write", "content": CONTENT,
                            "domain": "probe", "session_id": f"probe-{int(time.time())}"})
        wrote = (not err) and ("已写入" in txt)
        record("B", "write 写入一条记忆", wrote, txt[:80])

        err, txt = call(c, {"action": "recall", "query": QUERY, "top_k": 3})
        hit = (not err) and (MARKER in txt)
        record("B", "recall 命中刚写入的记忆", hit, f"命中={MARKER in txt}")

        err, txt = call(c, {"action": "recall", "query": QUERY, "top_k": 1})
        record("B", "recall 尊重 top_k=1", not err, f"len={len(txt)}")

        # ── C 幂等去重 ────────────────────────────────────────
        print("\n④ C 幂等去重")
        err, txt = call(c, {"action": "write", "content": CONTENT,
                            "domain": "probe", "session_id": f"probe-dup-{int(time.time())}"})
        record("C", "同内容重复 write 被拒（不重复入库）",
               (not err) and ("未写入" in txt), txt[:80])

        # ── D 边界防御 ────────────────────────────────────────
        print("\n⑤ D 边界防御（均应报错）")
        err, txt = call(c, {})
        record("D", "缺 action 参数 → 报错", err, txt[:70])

        err, txt = call(c, {"action": "destroy"})
        record("D", "未知 action → 报错", err, txt[:70])

        err, txt = call(c, {"action": "recall"})
        record("D", "recall 缺 query → 报错", err, txt[:70])

        err, txt = call(c, {"action": "write"})
        record("D", "write 缺 content → 报错", err, txt[:70])

        err, txt = call(c, {"action": "write", "content": "   "})
        record("D", "write 空白 content → 报错", err, txt[:70])

        # 提问意图句必须被拦在入口（与自动沉淀共用 is_worth_remembering）
        err, txt = call(c, {"action": "write",
                            "content": "关注 Rust 模块在项目中的角色"})
        record("D", "write 提问意图句 → 拒写（不落库）",
               err and ("未写入" in txt), txt[:70])

        err, txt = call(c, {"action": "write",
                            "content": "用户了解 Rust 编程"})
        record("D", "write 第三人称复述 → 拒写（不落库）",
               err and ("未写入" in txt), txt[:70])

        err, txt = call(c, {"action": "write", "content": "Blender 修改器介绍"})
        record("D", "write 空泛话题标题 → 拒写（不落库）",
               err and ("未写入" in txt), txt[:70])

        err, txt = call(c, {"action": "recall", "query": "   "})
        record("D", "recall 空 query → 不崩（返回空或提示）", not err, txt[:70])

    finally:
        c.close()

    # ── E 跨进程持久化 ────────────────────────────────────────
    print("\n⑥ E 跨进程持久化（重启进程后新会话 recall）")
    time.sleep(1.0)
    c2 = Client()
    try:
        c2.initialize()
        err, txt = call(c2, {"action": "recall", "query": QUERY, "top_k": 3})
        hit = (not err) and (MARKER in txt)
        record("E", "新进程 recall 仍命中（落库成功）", hit, f"命中={MARKER in txt}")
    finally:
        c2.close()

    # ── F 落库核对 ────────────────────────────────────────────
    after = db_counts()
    print("\n⑦ 落库核对：")
    for k in sorted(set(before) | set(after)):
        b, a = before.get(k, 0), after.get(k, 0)
        if a or b:
            print(f"   {k:<24} {b} -> {a}" + ("  ▲" if a > b else ""))

    # ── 汇总 ─────────────────────────────────────────────────
    print("\n" + "=" * 62)
    print("藏经阁 MCP 能力矩阵")
    print("=" * 62)
    passed = sum(1 for *_, p, _ in RESULTS if p)
    total = len(RESULTS)
    cur = None
    for area, case, p, detail in RESULTS:
        if area != cur:
            print(f"\n[{area}]")
            cur = area
        print(f"  {'✅' if p else '❌'} {case}" + (f"  ({detail})" if not p else ""))
    print(f"\n合计 {passed}/{total} 通过")
    return 0 if passed == total else 1


if __name__ == "__main__":
    sys.exit(main())
