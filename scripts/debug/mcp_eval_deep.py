#!/usr/bin/env python3
"""MCP 端到端评测探针（真实 stdio JSON-RPC，无 mock）。

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti python3 scripts/debug/mcp_eval_deep.py

覆盖范围见文件头 docstring。BIN 可用环境变量 SUBHUTI_BIN 覆盖，
默认回落到仓库内 target/debug/subhuti。
"""
#!/usr/bin/env python3
"""Probe #3：深挖上一轮评测的盲区。

覆盖：
  1. subhuti_skill_run 成功 / 失败（上一轮完全没测到）
  2. expert_id 强制路由（schema 声明了但未验证）
  3. system_prompt 覆盖
  4. 超长输入 / emoji / 边界字符
  5. 并发下 progressToken 隔离（通知必须归属正确的请求）
  6. 较大并发（6 个）仍保序无串扰
"""
import json
import subprocess
import threading
import time

import os
BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")
RESULTS = []


def ok(b):
    return "PASS ✓" if b else "FAIL ✗"


def rec(name, passed, detail=""):
    RESULTS.append((name, passed, detail))
    print(f"  [{ok(passed)}] {name}" + (f"  — {detail}" if detail else ""))


class Client:
    def __init__(self):
        self.proc = subprocess.Popen([BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True, bufsize=1)
        self._id = 0
        self._resp = {}
        self._prog = []          # list[(token, message)]
        self._ev = threading.Condition()
        self._stderr = []
        threading.Thread(target=self._r, daemon=True).start()
        threading.Thread(target=self._e, daemon=True).start()

    def _e(self):
        while True:
            ln = self.proc.stderr.readline()
            if not ln:
                break
            self._stderr.append(ln.rstrip())

    def _r(self):
        # 用显式 readline 循环（而非 for-in 迭代），与 eval.py 一致，
        # 避免文件对象迭代的读前缓冲影响时效性观察。
        while True:
            ln = self.proc.stdout.readline()
            if not ln:
                break
            ln = ln.strip()
            if not ln:
                continue
            try:
                m = json.loads(ln)
            except json.JSONDecodeError:
                continue
            with self._ev:
                if isinstance(m, dict) and m.get("method") == "notifications/progress":
                    p = m.get("params") or {}
                    self._prog.append((p.get("progressToken"), p.get("message", "")))
                elif isinstance(m, dict) and m.get("id") is not None:
                    self._resp[m["id"]] = m
                self._ev.notify_all()

    def send(self, o):
        self.proc.stdin.write(json.dumps(o) + "\n")
        self.proc.stdin.flush()

    def notify(self, m, p=None):
        self.send({"jsonrpc": "2.0", "method": m, "params": p or {}})

    def req(self, m, p=None, timeout=300):
        self._id += 1
        rid = self._id
        self.send({"jsonrpc": "2.0", "id": rid, "method": m, "params": p or {}})
        dl = time.time() + timeout
        with self._ev:
            while rid not in self._resp:
                r = dl - time.time()
                if r <= 0:
                    raise TimeoutError(m)
                self._ev.wait(r)
            return self._resp.pop(rid)

    def call(self, name, args, token=None, timeout=400):
        self._id += 1
        rid = self._id
        p = {"name": name, "arguments": args}
        if token is not None:
            p["_meta"] = {"progressToken": token}
        t0 = time.time()
        self.send({"jsonrpc": "2.0", "id": rid, "method": "tools/call", "params": p})
        dl = t0 + timeout
        with self._ev:
            while rid not in self._resp:
                r = dl - time.time()
                if r <= 0:
                    raise TimeoutError(name)
                # ⚠️ 必须 wait 释放锁再等：否则退化为「持锁忙等」，
                # 读取线程永远拿不到锁写入响应 → 必然超时（本探针曾因此假报 skill_run 卡死）
                self._ev.wait(r)
            return self._resp.pop(rid), time.time() - t0

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        try:
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.terminate()


def txt(r):
    try:
        return r["result"]["content"][0]["text"]
    except Exception:
        return json.dumps(r)[:250]


def err(r):
    return r.get("result", {}).get("isError")


def main():
    c = Client()
    c.req("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                         "clientInfo": {"name": "p3", "version": "1"}})
    c.notify("notifications/initialized")

    # ── 1. skill_run（上一轮盲区）─────────────────────────────
    print("\n=== 1. subhuti_skill_run ===")
    r, dt = c.call("subhuti_skill_run", {"skill_id": "rust-chat", "args": "用一句话说明什么是所有权"})
    t = txt(r)
    rec("skill_run 合法技能 → 成功", err(r) is False and len(t) > 10, f"{dt:.1f}s · {t[:70]}")
    r, dt = c.call("subhuti_skill_run", {"skill_id": "不存在的技能", "args": "x"})
    rec("skill_run 未知技能 → isError=true 且文案非空",
        err(r) is True and len(txt(r)) > 12, f"{txt(r)[:80]}")
    r, dt = c.call("subhuti_skill_run", {})
    rec("skill_run 缺 skill_id → isError=true", err(r) is True, f"{txt(r)[:70]}")
    r, dt = c.call("subhuti_skill_run", {"skill_id": "rust-chat", "args": {"not": "a string"}})
    rec("skill_run args 传对象 → 明确报错（不再空错误）",
        err(r) is True and "❌ 工具执行失败: " != txt(r)[:11], f"{txt(r)[:90]}")

    # ── 2. expert_id 强制路由 ──────────────────────────────
    print("\n=== 2. expert_id 强制路由 ===")
    # ⚠️ 显式传 session_id（eval-* 前缀），否则服务端生成随机 UUID 会话，
    # 沉淀写出的记忆无法被清理脚本按路径特征定位。两次调用用不同 session，
    # 避免粘性影响第 2 次「专家不存在」的判定。
    r, dt = c.call("subhuti_chat", {"message": "你好呀", "expert_id": "rust-expert",
                                   "session_id": "eval-deep-e1"}, token="e1")
    t = txt(r)
    rec("显式 expert_id=rust-expert → 命中该专家",
        err(r) is False and "Rust 编程专家" in t, f"{dt:.1f}s · {t[:70]}")
    r, dt = c.call("subhuti_chat", {"message": "你好呀", "expert_id": "no-such-expert",
                                   "session_id": "eval-deep-e2"}, token="e2")
    rec("expert_id 不存在 → 不静默回落（报失败或明确提示）",
        err(r) is True or "未匹配" in txt(r), f"isError={err(r)} · {txt(r)[:80]}")

    # ── 3. system_prompt 覆盖 ──────────────────────────────
    print("\n=== 3. system_prompt 覆盖 ===")
    # ⚠️ message 必须带领域 tag（含 "rust"），否则会先被「零命中不兜底」拦在专家之外，
    # system_prompt 根本没机会生效 —— 这会误报成「system_prompt 不生效」。
    r, dt = c.call("subhuti_chat", {
        "message": "用 rust 回答：只回复四个字，不要任何其他内容",
        "system_prompt": "你必须只输出「山高水长」四个字，不要输出任何其他内容。",
        "session_id": "eval-deep-s1",
    }, token="s1")
    rec("system_prompt 生效", err(r) is False and "山高水长" in txt(r),
        f"{dt:.1f}s · {txt(r)[:60]}")

    # ── 4. 输入边界 ──────────────────────────────
    print("\n=== 4. 输入边界 ===")
    r, dt = c.call("subhuti_chat", {"message": "🎬" * 40 + " 帮我看看 blender 动画", "session_id": "edge-emoji"}, token="b1")
    rec("emoji 输入不崩", "error" not in r and r.get("result") is not None, f"{dt:.1f}s · isError={err(r)}")
    big = "请介绍下 Rust 的模块系统。" + "补充说明：" * 400
    r, dt = c.call("subhuti_chat", {"message": big, "session_id": "edge-long"}, token="b2")
    rec("超长输入（约 2.6k 字）不崩", r.get("result") is not None, f"{dt:.1f}s · isError={err(r)}")
    r, dt = c.call("subhuti_chat", {"message": "   ", "session_id": "edge-blank"}, token="b3")
    rec("纯空白输入 → 有明确响应（不挂起）", r.get("result") is not None or "error" in r,
        f"isError={err(r)} · {txt(r)[:70]}")

    c.close()

    # ── 5. 并发下 progressToken 隔离 ──────────────────────────────
    print("\n=== 5. 并发 progressToken 隔离 ===")
    c2 = Client()
    c2.req("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                          "clientInfo": {"name": "p3b", "version": "1"}})
    c2.notify("notifications/initialized")
    with c2._ev:
        c2._prog.clear()

    outs = {}
    lock = threading.Lock()

    def fire(i, msg, tok):
        try:
            r, _ = c2.call("subhuti_chat", {"message": msg, "session_id": f"iso-{i}"}, token=tok)
            with lock:
                outs[i] = r
        except Exception as e:
            with lock:
                outs[i] = {"error": str(e)}

    ths = [
        threading.Thread(target=fire, args=(0, "介绍一下 rust 的模块系统", "TOKEN-A")),
        threading.Thread(target=fire, args=(1, "介绍一下 blender 的修改器", "TOKEN-B")),
    ]
    for t in ths:
        t.start()
    for t in ths:
        t.join()

    with c2._ev:
        prog = list(c2._prog)
    tokens = {tk for tk, _ in prog}
    rec("两个并发请求均返回", len(outs) == 2, f"返回 {len(outs)}/2")
    rec("进度通知的 token 只含本次订阅的 TOKEN-A/TOKEN-B",
        tokens.issubset({"TOKEN-A", "TOKEN-B"}) and len(tokens) > 0, f"tokens={sorted(str(x) for x in tokens)}")
    both = all(any(tk == t for tk, _ in prog) for t in ("TOKEN-A", "TOKEN-B"))
    rec("两个请求各自都收到了进度（未互相吞掉）", both,
        f"A={sum(1 for tk,_ in prog if tk=='TOKEN-A')} 条, B={sum(1 for tk,_ in prog if tk=='TOKEN-B')} 条")
    c2.close()

    # ── 6. 较大并发 ──────────────────────────────
    print("\n=== 6. 6 路并发只读调用 ===")
    c3 = Client()
    c3.req("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                          "clientInfo": {"name": "p3c", "version": "1"}})
    c3.notify("notifications/initialized")
    got = {}
    lock3 = threading.Lock()

    def fire3(i):
        name = ["subhuti_list_experts", "subhuti_skill_list", "subhuti_match_expert"][i % 3]
        arg = {"message": "rust"} if name == "subhuti_match_expert" else {}
        try:
            r, _ = c3.call(name, arg, token=f"T{i}")
            with lock3:
                got[i] = r
        except Exception as e:
            with lock3:
                got[i] = {"error": str(e)}

    t3 = [threading.Thread(target=fire3, args=(i,)) for i in range(6)]
    for t in t3:
        t.start()
    for t in t3:
        t.join()
    rec("6 路并发全部返回", len(got) == 6 and all(got.get(i) and "error" not in got[i] for i in range(6)),
        f"返回 {len(got)}/6")
    rec("并发响应内容类型正确",
        all(("专家" in txt(got[i]) or "技能" in txt(got[i])) for i in range(6)),
        "list/skill/match 内容未混淆")
    c3.close()

    print("\n" + "=" * 60)
    passed = sum(1 for _, p, _ in RESULTS if p)
    print(f"总计 {passed}/{len(RESULTS)} 项通过")
    for n, p, d in RESULTS:
        if not p:
            print(f"  ✗ {n} — {d}")
    print("=" * 60)


if __name__ == "__main__":
    main()
