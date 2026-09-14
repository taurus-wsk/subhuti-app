#!/usr/bin/env python3
"""MCP 端到端评测探针（真实 stdio JSON-RPC，无 mock）。

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti python3 scripts/debug/mcp_eval.py

覆盖范围见文件头 docstring。BIN 可用环境变量 SUBHUTI_BIN 覆盖，
默认回落到仓库内 target/debug/subhuti。
"""
#!/usr/bin/env python3
"""MCP 端到端评测（subhuti）——协议合规 + 五工具契约 + 错误语义 + 进度订阅 + 粘性 + 并发。

只走真实 stdio JSON-RPC（MCP 2024-11-05），不做任何 mock。
"""
import json
import subprocess
import threading
import time

import os
BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")


class Client:
    """单连接 MCP 客户端：后台线程读，按 id 分发响应，progress 通知单独收集。"""

    def __init__(self, tag="c"):
        self.tag = tag
        self.proc = subprocess.Popen(
            [BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1)
        self._id = 0
        self._lock = threading.Lock()
        self._resp = {}          # id -> response
        self._ev = threading.Condition()
        self._progress = []      # list[(id_key, message)] —— 按窗口收集
        self._prog_key = None
        self._stderr_tail = []
        self._stop = False
        threading.Thread(target=self._reader, daemon=True).start()
        threading.Thread(target=self._drain_stderr, daemon=True).start()

    def _drain_stderr(self):
        for ln in self.proc.stderr:
            self._stderr_tail.append(ln.rstrip())
            if len(self._stderr_tail) > 200:
                self._stderr_tail.pop(0)

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
                if m.get("method") == "notifications/progress":
                    self._progress.append(((m.get("params") or {}).get("message", "")))
                elif "id" in m and m["id"] is not None:
                    self._resp[m["id"]] = m
                self._ev.notify_all()

    def send(self, o):
        self.proc.stdin.write(json.dumps(o) + "\n")
        self.proc.stdin.flush()

    def notify(self, method, params=None):
        self.send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def request(self, method, params=None, timeout=400):
        with self._lock:
            self._id += 1
            rid = self._id
        self.send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}})
        dl = time.time() + timeout
        with self._ev:
            while rid not in self._resp:
                rem = dl - time.time()
                if rem <= 0:
                    raise TimeoutError(f"id={rid} method={method}")
                self._ev.wait(rem)
            return self._resp.pop(rid)

    def call_tool(self, name, arguments, token=None, timeout=400):
        """返回 (response, progress_msgs, elapsed)。token 为 None 表示不订阅进度。"""
        with self._lock:
            self._id += 1
            rid = self._id
        params = {"name": name, "arguments": arguments}
        if token is not None:
            params["_meta"] = {"progressToken": token}
        with self._ev:
            self._progress.clear()          # 本请求独用窗口（串行调用才能保证干净）
        t0 = time.time()
        self.send({"jsonrpc": "2.0", "id": rid, "method": "tools/call", "params": params})
        dl = t0 + timeout
        with self._ev:
            while rid not in self._resp:
                rem = dl - time.time()
                if rem <= 0:
                    raise TimeoutError(f"tool={name}")
                self._ev.wait(rem)
            resp = self._resp.pop(rid)
            prog = list(self._progress)
            self._progress.clear()
        return resp, prog, time.time() - t0

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.terminate()


def ok(b):
    return "PASS ✓" if b else "FAIL ✗"


RESULTS = []


def record(section, name, passed, detail=""):
    RESULTS.append((section, name, passed, detail))
    print(f"  [{ok(passed)}] {name}" + (f"  — {detail}" if detail else ""))


def text_of(resp):
    try:
        return resp["result"]["content"][0]["text"]
    except Exception:
        return json.dumps(resp)[:300]


def is_err(resp):
    return resp.get("result", {}).get("isError")


def main():
    c = Client()

    # ── 1. 生命周期与协议合规 ─────────────────────────────
    print("\n=== 1. 生命周期 / 协议合规 ===")
    r = c.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                 "clientInfo": {"name": "eval", "version": "1.0"}})
    res = r.get("result", {})
    record("lifecycle", "initialize 回显协议版本", res.get("protocolVersion") == "2024-11-05",
           f"version={res.get('protocolVersion')}")
    record("lifecycle", "声明 tools 能力", "tools" in res.get("capabilities", {}),
           f"caps={list(res.get('capabilities', {}).keys())}")
    record("lifecycle", "serverInfo 完整", bool(res.get("serverInfo", {}).get("name")),
           f"serverInfo={res.get('serverInfo')}")
    # instructions 是引导客户端主动使用藏经阁记忆的关键字段，缺失则退化为"手动提醒才用"
    ins = res.get("instructions") or ""
    record("lifecycle", "下发 instructions 使用说明（含 recall/write 引导）",
           bool(ins) and "subhuti_memory" in ins and "recall" in ins,
           f"instructions len={len(ins)}")
    c.notify("notifications/initialized")
    pong = c.request("ping")
    record("lifecycle", "ping 往返", "result" in pong)

    # ── 2. 工具发现 ─────────────────────────────
    print("\n=== 2. tools/list 工具发现 ===")
    tl = c.request("tools/list").get("result", {}).get("tools", [])
    names = [t["name"] for t in tl]
    expect = {"subhuti_chat", "subhuti_list_experts", "subhuti_match_expert",
              "subhuti_skill_list", "subhuti_skill_run", "subhuti_memory"}
    record("discovery", "工具数量 = 6", len(tl) == 6, f"names={names}")
    record("discovery", "工具名齐备", set(names) == expect,
           f"缺失={expect - set(names)} 多出={set(names) - expect}")
    schemas_ok = all(isinstance(t.get("inputSchema"), dict)
                     and t["inputSchema"].get("type") == "object"
                     and isinstance(t.get("description"), str) and t["description"]
                     for t in tl)
    record("discovery", "每个工具含合法 inputSchema(object) 与非空 description", schemas_ok)
    req_ok = all("required" in t["inputSchema"] for t in tl
                 if t["name"] in ("subhuti_chat", "subhuti_match_expert", "subhuti_skill_run"))
    record("discovery", "必填参数在 schema 中显式声明", req_ok)

    # ── 3. 通用错误语义 ─────────────────────────────
    print("\n=== 3. 错误语义 / 边界 ===")
    um = c.request("no/such/method")
    record("errors", "未知方法 → JSON-RPC -32601", um.get("error", {}).get("code") == -32601,
           f"error={um.get('error')}")
    # 无 id 的未知通知：应被静默忽略，且不得破坏协议流（随后的 ping 仍须正常）
    c.notify("no/such/notification")
    still_alive = "result" in c.request("ping")
    record("errors", "未知通知（无 id）静默忽略且不破坏协议流", still_alive)

    resp, _, _ = c.call_tool("subhuti_chat", {})
    record("errors", "缺必填 message → isError=true", is_err(resp) is True,
           f"text={text_of(resp)[:80]}")
    resp, _, _ = c.call_tool("subhuti_match_expert", {"query": "rust"})  # 旧参数名，应报缺 message
    record("errors", "match_expert 用错参数名 → isError=true", is_err(resp) is True,
           f"text={text_of(resp)[:80]}")
    resp, _, _ = c.call_tool("subhuti_no_such_tool", {})
    record("errors", "未知工具 → isError=true", is_err(resp) is True,
           f"text={text_of(resp)[:80]}")

    # ── 4. 只读工具 ─────────────────────────────
    print("\n=== 4. 只读工具 ===")
    resp, _, dt = c.call_tool("subhuti_list_experts", {})
    txt = text_of(resp)
    n_expert = txt.count("- ")
    record("tools", "list_experts 返回专家清单", is_err(resp) is False and n_expert > 0,
           f"{n_expert} 位 · {dt*1000:.0f}ms")
    resp, _, dt = c.call_tool("subhuti_skill_list", {})
    txt = text_of(resp)
    record("tools", "skill_list 返回技能清单", is_err(resp) is False and ("可用技能" in txt or "无可用技能" in txt),
           f"'可用技能'={'可用技能' in txt} · {dt*1000:.0f}ms")
    resp, _, dt = c.call_tool("subhuti_match_expert", {"message": "帮我用 rust 写个函数并编译"})
    txt = text_of(resp)
    record("tools", "match_expert(rust) 命中领域", is_err(resp) is False and "Rust" in txt,
           f"→ {txt.strip().splitlines()[-1][:70] if txt.strip() else ''}")
    resp, _, dt = c.call_tool("subhuti_match_expert", {"message": "今天天气怎么样"})
    txt = text_of(resp)
    record("tools", "match_expert(领域外) 不误命中", "未匹配到专家" in txt or "Rust" not in txt,
           f"→ {txt.strip()[:70]}")

    # ── 5. 进度订阅（opt-in / opt-out）─────────────────────────────
    print("\n=== 5. 进度通知订阅语义 ===")
    # ⚠️ 必须显式传 session_id：不传时服务端会生成随机 UUID，会话沉淀写出的记忆
    # 就带一个每次跑都变的 session 路径，清理脚本无法靠路径特征定位（实测因此
    # 在库里留下过 4 条查不到来源的「Rust 语言」类残留）。统一用 eval-* 前缀。
    resp, prog_no, dt = c.call_tool("subhuti_chat",
                                    {"message": "介绍一下 rust 模块的职责",
                                     "session_id": "eval-progress-1"},
                                    token=None)
    record("progress", "未订阅 → 零条 progress 通知", len(prog_no) == 0,
           f"收到 {len(prog_no)} 条 · isError={is_err(resp)} · {dt:.1f}s")

    resp, prog_yes, dt = c.call_tool("subhuti_chat",
                                     {"message": "介绍一下 rust 模块的职责",
                                      "session_id": "eval-progress-2"},
                                     token="eval-token-1")
    has_prog = len(prog_yes) > 0
    record("progress", "订阅（带 progressToken）→ 收到 progress 通知", has_prog,
           f"收到 {len(prog_yes)} 条 · {dt:.1f}s")
    phases = sorted({p.split("】")[1].split(" ")[0].strip("[]")
                     for p in prog_yes if "【" in p and "】" in p})
    record("progress", "progress 内容含阶段标签", len(phases) > 0, f"phases={phases}")
    print("     进度样本:", prog_yes[:6])

    # ── 6. 聊天成败语义 ─────────────────────────────
    print("\n=== 6. subhuti_chat 成败语义 ===")
    # 两次调用必须用**不同** session：共用同一 session 会让第 2 次（领域外提问）
    # 因会话粘性继承 rust 领域，断言 isError=true 就会失败。
    resp, _, dt = c.call_tool("subhuti_chat",
                              {"message": "介绍一下这个项目里 rust 模块的职责",
                               "session_id": "eval-chatsem-1"},
                              token="t2")
    txt = text_of(resp)
    record("chat", "领域内请求 → isError=false", is_err(resp) is False, f"{dt:.1f}s")
    record("chat", "返回含 [meta] 专家链/耗时/trace", "[meta]" in txt,
           txt.split("[meta]")[-1][:90].strip() if "[meta]" in txt else txt[:80])

    resp, _, dt = c.call_tool("subhuti_chat",
                              {"message": "今天天气怎么样",
                               "session_id": "eval-chatsem-2"},
                              token="t3")
    record("chat", "领域外请求 → isError=true（不兜底）", is_err(resp) is True,
           f"{dt:.1f}s · {text_of(resp)[:90]}")

    c.close()

    # ── 7. 会话粘性（同一连接、同一 session_id）──────────────────
    print("\n=== 7. 会话粘性（多轮）===")
    c2 = Client()
    c2.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                              "clientInfo": {"name": "eval2", "version": "1.0"}})
    c2.notify("notifications/initialized")
    sid = "eval-sticky-1"
    r1, _, _ = c2.call_tool("subhuti_chat", {"message": "用 rust 写一个函数", "session_id": sid}, token="s1")
    t1 = text_of(r1)
    record("sticky", "轮1（rust 域）已路由到专家、未判领域外",
           "未匹配到相关领域专家" not in t1, f"isError={is_err(r1)}")

    # ⚠️ 粘性是否生效的**正确判据**是「有没有被判领域外」，而不是「响应里有没有 [meta]」：
    # 失败的请求走 Err 分支、不拼 meta 行，若拿 [meta] 判断会把「粘性生效但执行失败」
    # 误判成「粘性失效」。判据必须落在路由结果上。
    r2, _, _ = c2.call_tool("subhuti_chat", {"message": "那再补充一下错误处理", "session_id": sid}, token="s2")
    t2 = text_of(r2)
    boundary2 = "未匹配到相关领域专家" in t2
    record("sticky", "轮2（承接追问）未被判领域外 → 粘性延续生效",
           not boundary2, f"isError={is_err(r2)} · {t2[:50]}")

    r3, _, _ = c2.call_tool("subhuti_chat", {"message": "今天天气怎么样", "session_id": sid}, token="s3")
    t3 = text_of(r3)
    record("sticky", "显式跨域问题仍走边界提示（粘性不越权兜底）",
           "领域" in t3 or is_err(r3) is True, f"{t3[:70]}")
    c2.close()

    # ── 8. 并发（同一连接多请求乱序返回，按 id 必须能对上）──────────
    print("\n=== 8. 并发（3 个只读调用同时发）===")
    c3 = Client()
    c3.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                              "clientInfo": {"name": "eval3", "version": "1.0"}})
    c3.notify("notifications/initialized")
    got = {}
    lock = threading.Lock()

    def fire(i, tool, arg):
        r, _, _ = c3.call_tool(tool, arg, token=f"p{i}")
        with lock:
            got[i] = r

    ths = [
        threading.Thread(target=fire, args=(0, "subhuti_list_experts", {})),
        threading.Thread(target=fire, args=(1, "subhuti_skill_list", {})),
        threading.Thread(target=fire, args=(2, "subhuti_match_expert", {"message": "blender 建模"})),
    ]
    for t in ths:
        t.start()
    for t in ths:
        t.join()
    record("concurrency", "3 个并发请求全部返回且 id 无串扰", len(got) == 3 and all(got.get(i) for i in range(3)),
           f"返回 {len(got)}/3")
    record("concurrency", "并发响应内容各自正确",
           ("专家" in text_of(got[0])) and ("技能" in text_of(got[1])) and is_err(got[2]) is False,
           "list/skill/match 内容互不混淆")
    c3.close()

    # ── 汇总 ─────────────────────────────
    print("\n" + "=" * 60)
    total = len(RESULTS)
    passed = sum(1 for _, _, p, _ in RESULTS if p)
    print(f"总计 {passed}/{total} 项通过")
    for sec in dict.fromkeys(s for s, _, _, _ in RESULTS):
        ps = [p for s, _, p, _ in RESULTS if s == sec]
        print(f"  {sec:12s} {sum(ps)}/{len(ps)}")
    fails = [(s, n, d) for s, n, p, d in RESULTS if not p]
    if fails:
        print("\n未通过：")
        for s, n, d in fails:
            print(f"  ✗ [{s}] {n} — {d}")
    print("=" * 60)


if __name__ == "__main__":
    main()
