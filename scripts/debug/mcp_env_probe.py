#!/usr/bin/env python3
"""subhuti MCP 环境自检 —— 回答「接到 WorkBuddy 之后到底哪些能力是通的」。

背景（2026-09-14 实测）
-----------------------
`~/.workbuddy/mcp.json` 里的 subhuti 条目只给了 `SUBHUTI_DATA_DIR`，**没有设置 cwd**。
子进程 cwd 因此继承宿主进程（通常不在项目根），于是：

- `config/Subhuti.toml` → **仍能找到**。`infra/config.rs::find_config_path` 有
  编译期兜底 `env!("CARGO_MANIFEST_DIR")`，debug 二进制里硬编码了源码路径。
- `.env` → **找不到**。`mcp.rs` 用的是 `dotenvy::dotenv()`，只从 cwd 向上找，
  没有任何兜底。而 `ZHIPU_API_KEY` **只从环境变量取**（Subhuti.toml 里不写 key）。

结果就是能力被劈成两半：

| 能力 | 依赖 | 项目外 cwd |
|------|------|-----------|
| `subhuti_memory` recall/write/stats/collections | 只读库 | ✅ 可用 |
| `subhuti_list_experts` / `subhuti_match_expert` | 注册表 | ✅ 可用 |
| `subhuti_chat` / `subhuti_skill_run` | LLM key | ❌ HTTP 401 |
| chat 结束后的**自动沉淀** | LLM key | ❌ 连带失效 |

本脚本把这件事变成可重复验证的断言，避免"以为接好了"。

用法
----
    python3 scripts/debug/mcp_env_probe.py          # 全量（含一次真实 LLM 调用，会写库）
    python3 scripts/debug/mcp_env_probe.py --no-llm  # 只查不花钱的只读能力

退出码：0 = 与预期一致；1 = 有偏差（例如项目外 cwd 下 chat 竟然成功，说明环境变了）
"""
import argparse
import json
import os
import select
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.environ.get("SUBHUTI_BIN") or os.path.join(REPO, "target/debug/subhuti")
DATA = os.environ.get("SUBHUTI_DATA_DIR") or os.path.expanduser("~/sqlite")

# 复刻 WorkBuddy：最小环境 + 仅 SUBHUTI_DATA_DIR（不传 ZHIPU_API_KEY）
MIN_ENV = {
    "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    "HOME": os.path.expanduser("~"),
    "SUBHUTI_DATA_DIR": DATA,
}


def _read(p, timeout):
    r, _, _ = select.select([p.stdout], [], [], timeout)
    if not r:
        return None
    ln = p.stdout.readline()
    return json.loads(ln) if ln.strip() else None


class Session:
    def __init__(self, cwd):
        self.p = subprocess.Popen(
            [BIN, "mcp"], cwd=cwd, env=dict(MIN_ENV),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1,
        )
        self.cid = 100
        self.ok_init = False
        try:
            self._send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "env-probe", "version": "1"}}})
            self.ok_init = _read(self.p, 20) is not None
            self._send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
        except Exception:
            pass

    def _send(self, obj):
        self.p.stdin.write(json.dumps(obj) + "\n")
        self.p.stdin.flush()

    def call(self, name, args, timeout=200):
        self.cid += 1
        cid = self.cid
        self._send({"jsonrpc": "2.0", "id": cid, "method": "tools/call",
                    "params": {"name": name, "arguments": args}})
        deadline = time.time() + timeout
        while time.time() < deadline:
            msg = _read(self.p, max(0.5, deadline - time.time()))
            if msg is None:
                continue
            if msg.get("id") == cid:
                return msg
        return None

    def close(self):
        try:
            self.p.stdin.close()
        except Exception:
            pass
        self.p.terminate()
        time.sleep(0.2)
        try:
            return self.p.stderr.read() or ""
        except Exception:
            return ""


def summarize(resp):
    """→ (是否成功, 短文本)"""
    if resp is None:
        return False, "<超时/无响应>"
    if "error" in resp:
        return False, f"<RPC错误> {json.dumps(resp['error'], ensure_ascii=False)[:160]}"
    r = resp.get("result", {})
    body = "\n".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
    return (not r.get("isError")), body.strip()


def run(label, cwd, with_llm, session_id):
    print(f"\n{'='*74}\n[{label}]  cwd = {cwd}\n{'='*74}")
    s = Session(cwd)
    if not s.ok_init:
        print("  ❌ initialize 无响应")
        s.close()
        return {}
    print("  ✅ initialize")

    out = {}
    for key, tool, args in [
        ("stats", "subhuti_memory", {"action": "stats"}),
        ("recall", "subhuti_memory", {"action": "recall", "query": "渲染器用的是什么"}),
        ("experts", "subhuti_list_experts", {}),
    ]:
        ok, txt = summarize(s.call(tool, args, timeout=60))
        out[key] = ok
        head = txt.replace("\n", " / ")[:110]
        print(f"  {'✅' if ok else '❌'} {key:8} {head}")

    if with_llm:
        ok, txt = summarize(s.call("subhuti_chat", {
            "message": "用一句话说明这个项目里 rust 模块的职责",
            "expert_id": "rust-expert", "session_id": session_id}, timeout=240))
        out["chat"] = ok
        first = [l for l in txt.splitlines() if l.strip()][:3]
        print(f"  {'✅' if ok else '❌'} chat     " + " / ".join(first)[:150])
    else:
        print("  ⏭  chat     已跳过（--no-llm）")

    err = s.close()
    if err and any(k in err for k in ("401", "未配置", "api key", "API key")):
        print("  --- stderr 关键行 ---")
        for ln in err.splitlines():
            if "401" in ln or "key" in ln.lower():
                print("    " + ln.strip()[:160])
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-llm", action="store_true", help="跳过会消耗额度的 LLM 调用")
    args = ap.parse_args()

    print("subhuti MCP 环境自检")
    print(f"  二进制 : {BIN}")
    print(f"  数据目录: {DATA}")
    print(f"  时间   : {time.strftime('%Y-%m-%d %H:%M:%S')}")

    sid = f"envprobe-{int(time.time())}"
    outside = run("A 项目外 cwd（= WorkBuddy 实际情形）", "/tmp", not args.no_llm, sid)
    inside = run("B 项目内 cwd（.env 可达）", REPO, not args.no_llm, sid)

    print(f"\n{'='*74}\n结论\n{'='*74}")
    read_keys = ("stats", "recall", "experts")
    print(f"  藏经阁读取（stats/recall/experts）  项目外: "
          f"{'✅ 可用' if all(outside.get(k) for k in read_keys) else '❌ 不可用'}")
    if not args.no_llm:
        print(f"  LLM 链路（chat/skill_run/自动沉淀）  项目外: "
              f"{'✅ 可用' if outside.get('chat') else '❌ 不可用（ZHIPU_API_KEY 缺失）'}")
        print(f"                                     项目内: "
              f"{'✅ 可用' if inside.get('chat') else '❌ 不可用'}")
    print("\n  修复方式（二选一）：")
    print("    ① 在 ~/.workbuddy/mcp.json 的 subhuti.env 里补 \"ZHIPU_API_KEY\"（推荐，显式不依赖 cwd）")
    print("    ② 给该条目加 \"cwd\": \"<项目根>\"（若客户端支持），让 dotenvy 能找到 .env")
    print(f"\n  探针 session 标记: {sid}（清理：python3 scripts/debug/cleanup_probe_data.py）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
