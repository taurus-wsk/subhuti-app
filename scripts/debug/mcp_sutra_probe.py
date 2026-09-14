#!/usr/bin/env python3
"""藏经阁记忆引擎 MCP 探针（真实 stdio JSON-RPC，无 mock）。

重点验证「运行中动态沉淀 → 下次召回」这个记忆闭环是否真的成立：
  轮次1：告诉它一条项目专属事实（静态知识库里绝对没有的）
  轮次2：新开会话/同会话追问这条事实，看能否召回

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti python3 scripts/debug/mcp_sutra_probe.py

环境变量：
    SUBHUTI_BIN         被测二进制，默认 target/debug/subhuti
    SUBHUTI_SUTRA_SQLITE   藏经阁库路径；缺省由 SUBHUTI_DATA_DIR 推导
"""
import json
import os
import sqlite3
import subprocess
import sys
import threading
import time

BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")


def sutra_db_path() -> str:
    p = os.environ.get("SUBHUTI_SUTRA_SQLITE")
    if p:
        return p
    data_dir = os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/.subhuti/data"))
    return os.path.join(data_dir, "sutra_library.sqlite")


def db_counts(path: str) -> dict:
    """读藏经阁各表行数。"""
    if not os.path.exists(path):
        return {}
    c = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    try:
        tables = [r[0] for r in c.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")]
        out = {}
        for t in tables:
            try:
                out[t] = c.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
            except sqlite3.Error:
                out[t] = -1
        return out
    finally:
        c.close()


class Client:
    def __init__(self):
        self.proc = subprocess.Popen(
            [BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1)
        self._id = 0
        self._lock = threading.Lock()
        self._resp = {}
        self._ev = threading.Condition()
        threading.Thread(target=self._reader, daemon=True).start()
        threading.Thread(target=self._drain, daemon=True).start()

    def _drain(self):
        for _ in self.proc.stderr:
            pass

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

    def request(self, method, params=None, timeout=300):
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


def call_tool(c, name, args, timeout=300):
    resp = c.request("tools/call", {"name": name, "arguments": args}, timeout=timeout)
    result = resp.get("result") or {}
    is_err = bool(result.get("isError"))
    text = ((result.get("content") or [{}])[0].get("text", "") or "")
    return is_err, text


def main() -> int:
    db = sutra_db_path()
    print(f"藏经阁库：{db}")
    print(f"被测二进制：{BIN}\n")

    before = db_counts(db)
    print("① 请求前各表行数：")
    for k, v in sorted(before.items()):
        if v or k in ("memory_nodes", "memory_edges", "kb_chunk", "domain_kb",
                      "memory_collections", "memory_snapshots"):
            print(f"   {k:<28} {v}")
    print()

    c = Client()
    try:
        c.request("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sutra-probe", "version": "1.0"},
        }, timeout=30)

        # 可用工具清单
        resp = c.request("tools/list", {}, timeout=30)
        tools = (resp.get("result") or {}).get("tools", [])
        print("② MCP 暴露的工具：")
        for t in tools:
            print(f"   - {t.get('name')}")
        print()

        # 技能清单（藏经阁是否作为 skill 暴露）
        is_err, text = call_tool(c, "subhuti_skill_list", {}, timeout=60)
        print("③ 技能清单（前 500 字）：")
        print("   " + (text[:500].replace("\n", "\n   ") if text else "(空)"))
        print()

        session = f"sutra-probe-{int(time.time())}"

        # 轮次1：注入一条项目专属事实
        fact = "我的 Blender 项目固定使用 Cycles 渲染器，采样值设为 128，输出格式统一 PNG。"
        print(f"④ 轮次1：注入事实 → {fact}")
        t0 = time.time()
        is_err, text = call_tool(c, "subhuti_chat", {
            "message": f"记住这个设定：{fact}",
            "session_id": session,
        })
        print(f"   {time.time() - t0:.1f}s  isError={is_err}")
        print("   回复（前 300 字）：" + (text[:300].replace("\n", "\n   ") or "(空)"))
        print()

        # 轮次2：同会话追问（检验召回）
        q = "我刚才说的渲染器和采样值是多少？"
        print(f"⑤ 轮次2：同会话追问 → {q}")
        t0 = time.time()
        is_err, text = call_tool(c, "subhuti_chat", {
            "message": q,
            "session_id": session,
        })
        print(f"   {time.time() - t0:.1f}s  isError={is_err}")
        print("   回复（前 600 字）：" + (text[:600].replace("\n", "\n   ") or "(空)"))
        print()

        # 轮次3：新会话追问（检验跨会话长期记忆，同一进程内）
        print(f"⑥ 轮次3：新会话追问（同进程）→ {q}")
        t0 = time.time()
        is_err, text = call_tool(c, "subhuti_chat", {
            "message": q,
            "session_id": f"{session}-fresh",
        })
        print(f"   {time.time() - t0:.1f}s  isError={is_err}")
        print("   回复（前 600 字）：" + (text[:600].replace("\n", "\n   ") or "(空)"))
        print(f"   >>> 同进程跨会话命中: {'✅' if ('Cycles' in text and '128' in text) else '❌'}")
    finally:
        c.close()

    # 轮次4：**换一个全新进程**再追问 —— 这才是真正的记忆闭环。
    #
    # 同一进程里内存态还留着沉淀结果，答对说明不了持久化；
    # 只有"进程重启后仍答得出来"，才能证明 沉淀→落库→启动回灌→召回 全链路通。
    print(f"\n⑥′ 轮次4：全新进程 + 新会话追问 → {q}")
    c2 = Client()
    try:
        c2.request("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sutra-probe", "version": "1.0"},
        }, timeout=30)
        t0 = time.time()
        is_err, text = call_tool(c2, "subhuti_chat", {
            "message": q,
            "session_id": f"{session}-restart",
        })
        print(f"   {time.time() - t0:.1f}s  isError={is_err}")
        print("   回复（前 600 字）：" + (text[:600].replace("\n", "\n   ") or "(空)"))
        hit = "Cycles" in text and "128" in text
        print(f"   >>> 跨进程持久化命中: {'✅' if hit else '❌'}")
    finally:
        c2.close()

    # 落库是异步的，等一下再查
    after = before
    for _ in range(20):
        time.sleep(0.5)
        after = db_counts(db)
        if after != before:
            break

    print("\n⑦ 请求后各表行数（▲ 表示有增长）：")
    for k in sorted(set(before) | set(after)):
        b, a = before.get(k, 0), after.get(k, 0)
        if a or b:
            mark = " ▲" if a > b else ""
            print(f"   {k:<28} {a}{mark}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
