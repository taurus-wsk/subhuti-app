#!/usr/bin/env python3
"""验证「WorkBuddy 把藏经阁当作纯记忆引擎」是否成立 —— 只看保存与搜索。

范围
----
只测 `subhuti_memory` 的 **write / recall / stats**，不碰 `subhuti_chat`
（chat 依赖 LLM key，属于另一条链路，本脚本刻意不涉及）。

为什么要单独测
--------------
`mcp_memory_probe.py` 测的是"能力矩阵是否可用"（工具能不能调通）；
本脚本测的是**"能不能当记忆引擎"**，即：

  1. 冷启动是否干净
  2. 写入是否可靠（含幂等 / 拒写意图句 / 拒写空白）
  3. 搜索是否排得准（字面 query）
  4. 搜索能否扛住**同义改写**（非字面重叠）—— 这是"记忆引擎"与"关键词搜索"的分水岭
  5. **跨进程持久化** —— 关掉进程重开，记忆还在（不能是内存态）
  6. 累积到 20 条后，早期记忆是否仍能准确召回
  7. 真实库里的既有记忆是否可直接搜到

运行姿势与 WorkBuddy 一致
--------------------------
- cwd = `/tmp`（项目外，复刻客户端行为）
- env 只有 `SUBHUTI_DATA_DIR`（不注入 ZHIPU_API_KEY —— 本链路不需要 LLM）

数据隔离
--------
主验证用**独立库** `/tmp/subhuti-memtest`（每次运行前重建），
不触碰 `/Users/hezenghui/sqlite` 里的真实记忆。
真实库只做**只读**召回验证。

用法
----
    python3 scripts/debug/mcp_memory_engine_probe.py
    python3 scripts/debug/mcp_memory_engine_probe.py --keep   # 保留测试库供人工查看
"""
import argparse
import json
import os
import select
import shutil
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BIN = os.environ.get("SUBHUTI_BIN") or os.path.join(REPO, "target/debug/subhuti")
TEST_DATA = "/tmp/subhuti-memtest"
REAL_DATA = os.environ.get("SUBHUTI_DATA_DIR") or os.path.expanduser("~/sqlite")
CWD = "/tmp"  # 复刻 WorkBuddy：项目外
# 由 --no-seed 填充；会注入每个子进程的环境
EXTRA_ENV = {}

RESULTS = []


def rec(group, name, ok, detail=""):
    RESULTS.append((group, name, ok, detail))
    print(f"  {'✅' if ok else '❌'} [{group}] {name}" + (f"  — {detail}" if detail else ""))


# ─────────────────────────── MCP 客户端 ───────────────────────────
class Mem:
    """一个 subhuti mcp 子进程 = 一次"客户端会话"。"""

    def __init__(self, data_dir):
        env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": os.path.expanduser("~"),
               "SUBHUTI_DATA_DIR": data_dir}
        env.update(EXTRA_ENV)
        self.p = subprocess.Popen([BIN, "mcp"], cwd=CWD, env=env, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  text=True, bufsize=1)
        self.cid = 0
        self._send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "mem-probe", "version": "1"}}})
        self.ok = self._read(25) is not None
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

    def _send(self, o):
        try:
            self.p.stdin.write(json.dumps(o) + "\n")
            self.p.stdin.flush()
        except Exception:
            pass

    def _read(self, timeout):
        r, _, _ = select.select([self.p.stdout], [], [], timeout)
        if not r:
            return None
        ln = self.p.stdout.readline()
        return json.loads(ln) if ln.strip() else None

    def call(self, args, timeout=60):
        self.cid += 1
        cid = self.cid
        self._send({"jsonrpc": "2.0", "id": cid, "method": "tools/call",
                    "params": {"name": "subhuti_memory", "arguments": args}})
        deadline = time.time() + timeout
        while time.time() < deadline:
            m = self._read(max(0.3, deadline - time.time()))
            if m and m.get("id") == cid:
                return m
        return None

    def close(self):
        try:
            self.p.stdin.close()
        except Exception:
            pass
        self.p.terminate()
        try:
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def body(resp):
    """→ (是否成功, 文本)"""
    if resp is None:
        return False, "<超时>"
    if "error" in resp:
        return False, f"<RPC错误> {json.dumps(resp['error'], ensure_ascii=False)[:200]}"
    r = resp.get("result", {})
    t = "\n".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
    return (not r.get("isError")), t.strip()


def write(m, content, domain="general"):
    ok, t = body(m.call({"action": "write", "content": content, "domain": domain}))
    return ok, t


def recall_titles(m, query, top_k=5):
    """召回结果 → (是否成功, [标题...], 原始文本)"""
    ok, t = body(m.call({"action": "recall", "query": query, "top_k": top_k}))
    if not ok:
        return False, [], t
    titles = []
    for line in t.splitlines():
        s = line.strip()
        if s[:1].isdigit() and ". **" in s:
            try:
                titles.append(s.split("**", 2)[1])
            except IndexError:
                pass
    return True, titles, t


def stat(m):
    ok, t = body(m.call({"action": "stats"}))
    return ok, t


def overlap_ratio(query, title):
    """query 的字符 2-gram 在 title 里的覆盖率。

    不能用 `query in title` 做判据：查询是「模型导出格式」而标题是
    「模型导出统一用 FBX 格式」，语义已命中却**没有连续子串**，
    子串判定会把它误报成失败（第一轮实测就踩了这个坑）。
    """
    q = "".join(query.split())
    t = "".join(title.split())
    if len(q) < 2:
        return 1.0 if q in t else 0.0
    grams = {q[i:i + 2] for i in range(len(q) - 1)}
    return sum(1 for g in grams if g in t) / len(grams)


def wait_stable(m, max_wait=30.0, need=3):
    """轮询 stats 直到节点数连续 need 次不变，返回稳定后的节点数。

    ⚠️ 必须等稳：冷启动种子是 `tokio::spawn` + 固定 sleep 800ms 之后才灌的
    （`composition_root.rs`），所以空库刚启动时 stats=0，约 1~2 秒后才变成 4 条。
    不等稳就断言，会得到"节点数对不上""检索结果漂移"这类**假失败**。
    """
    prev, stable, t0 = None, 0, time.time()
    while time.time() - t0 < max_wait:
        _, st = stat(m)
        n = num_field(st, "总节点数")
        if n == prev:
            stable += 1
            if stable >= need:
                return n
        else:
            stable = 0
        prev = n
        time.sleep(0.4)
    return prev if prev is not None else 0


def num_field(stats_text, label):
    for line in stats_text.splitlines():
        if label in line:
            digits = "".join(ch for ch in line if ch.isdigit())
            if digits:
                return int(digits)
    return None


# ─────────────────────────── 测试语料 ───────────────────────────
# 6 条基本事实（刻意避开真实库里已有的 Cycles/128/PNG 那几条，避免互相干扰）
FACTS = [
    ("渲染输出目录固定 /tmp/render_out", "blender", "渲染输出目录", "渲染结果存到哪个目录"),
    ("Blender 版本锁定 4.3.2，不随意升级", "blender", "Blender 版本", "用的是哪个 Blender"),
    ("模型导出统一用 FBX 格式", "blender", "模型导出格式", "导出模型用什么格式"),
    ("日志库选 tracing，不用 log", "rust", "日志库", "日志用什么库"),
    ("错误处理统一用 anyhow", "rust", "错误处理", "报错怎么处理"),
    ("数据库连接池上限设为 10", "rust", "连接池", "数据库连接数上限是多少"),
]
# 追加语料（把库撑到 20 条，看早期记忆会不会被稀释）
EXTRA = [
    ("前端状态管理用 Redux Toolkit", "general"),
    ("接口超时统一设 30 秒", "general"),
    ("测试数据放 /tmp/fixtures", "general"),
    ("打包前必须跑 cargo fmt", "rust"),
    ("CLI 参数解析用 clap", "rust"),
    ("异步任务统一走 tokio::spawn", "rust"),
    ("材质节点命名以 mat_ 开头", "blender"),
    ("导出前先应用所有修改器", "blender"),
    ("光照统一用三点布光", "blender"),
    ("渲染采样上限不超过 512", "blender"),
    ("模型面数控制在 50 万以内", "blender"),
    ("纹理统一 2K 分辨率", "blender"),
    ("提交信息用中文描述", "general"),
    ("仓库默认分支是 main", "general"),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", action="store_true", help="保留测试库")
    ap.add_argument("--no-seed", action="store_true",
                    help="给子进程设 SUBHUTI_DISABLE_SEED=1，验证冷启动种子可关闭")
    args = ap.parse_args()
    if args.no_seed:
        EXTRA_ENV["SUBHUTI_DISABLE_SEED"] = "1"

    os.makedirs(CWD, exist_ok=True)
    print("藏经阁「纯记忆引擎」验证")
    print(f"  二进制   : {BIN}")
    print(f"  测试库   : {TEST_DATA}（每轮重建）")
    print(f"  真实库   : {REAL_DATA}（只读）")
    print(f"  调用姿势 : cwd={CWD}（复刻 WorkBuddy）")
    print(f"  时间     : {time.strftime('%Y-%m-%d %H:%M:%S')}")

    # ── 阶段 0：重建测试库 ──
    if os.path.exists(TEST_DATA):
        shutil.rmtree(TEST_DATA)
    os.makedirs(TEST_DATA)

    m = Mem(TEST_DATA)
    if not m.ok:
        print("❌ initialize 失败，终止")
        return 1

    # ── 阶段 1：冷启动 ──
    print("\n=== 阶段 1 · 冷启动：空库态与异步种子 ===")
    ok, st = stat(m)
    n0 = num_field(st, "总节点数")
    rec("冷启动", "空库 stats 可读", ok, st.splitlines()[0] if st else "")
    rec("冷启动", "刚落盘时节点数=0", n0 == 0, f"总节点数={n0}")

    seed_n = wait_stable(m)
    print(f"  ℹ️  引擎稳定后节点数 = {seed_n}")
    if EXTRA_ENV.get("SUBHUTI_DISABLE_SEED") == "1":
        rec("冷启动", "已关闭种子（SUBHUTI_DISABLE_SEED=1）→ 稳定后仍为 0 条",
            seed_n == 0, f"实际 {seed_n}")
    else:
        rec("冷启动", f"稳定后自动出现 {seed_n} 条（冷启动种子）", True,
            "⚠️ 非用户写入，是 rust 专家静态知识，会混进召回结果")

    # ── 阶段 2：写入（保存） ──
    print("\n=== 阶段 2 · 写入（保存） ===")
    for content, dom, _q, _s in FACTS:
        ok, t = write(m, content, dom)
        rec("写入", f"写入「{content[:18]}…」", ok and t.startswith("✅"), t[:60])

    ok, t = write(m, FACTS[0][0], FACTS[0][1])
    rec("写入", "重复写入同一条 → 幂等不重复", ok and "未写入" in t, t[:60])

    ok, t = write(m, "关注 Blender 修改器的作用")
    rec("写入", "意图句「关注 X」→ 拒绝", (not ok) or ("未写入" in t) or ("疑似" in t), t[:70])

    ok, t = write(m, "   ")
    rec("写入", "空白 content → 拒绝", (not ok), t[:70])

    ok, st = stat(m)
    n1 = num_field(st, "总节点数")
    rec("写入", f"节点数 = 种子({seed_n}) + 写入({len(FACTS)})",
        n1 == seed_n + len(FACTS), f"期望 {seed_n + len(FACTS)}，实际 {n1}")

    # ── 阶段 3：搜索（字面） ──
    print("\n=== 阶段 3 · 搜索：字面关键词是否排第 1 ===")
    for content, _dom, kw, _syn in FACTS:
        ok, titles, _raw = recall_titles(m, kw)
        hit = bool(titles) and overlap_ratio(content, titles[0]) >= 0.5
        rec("字面召回", f"「{kw}」→ 正确答案排第 1", ok and hit,
            f"top1={titles[0] if titles else '(空)'}")

    # ── 阶段 4：搜索（同义改写，非字面重叠） ──
    print("\n=== 阶段 4 · 搜索：同义改写能否命中（记忆引擎的关键） ===")
    for content, _dom, _kw, syn in FACTS:
        ok, titles, _raw = recall_titles(m, syn)
        hit = bool(titles) and overlap_ratio(content, titles[0]) >= 0.5
        rec("同义召回", f"「{syn}」→ 正确答案排第 1", ok and hit,
            f"top1={titles[0] if titles else '(空)'}")

    # ── 阶段 5：top_k 与无关查询 ──
    print("\n=== 阶段 5 · top_k 与拒答 ===")
    ok, titles, _ = recall_titles(m, "连接池", top_k=1)
    rec("top_k", "top_k=1 只返回 1 条", ok and len(titles) == 1, f"实际 {len(titles)} 条")
    ok, titles, raw = recall_titles(m, "如何做红烧肉", top_k=5)
    rec("拒答", "无关查询不返回既有记忆", ok and not titles, f"返回 {len(titles)} 条 · {raw[:60]}")

    # ── 阶段 6：跨进程持久化 ──
    print("\n=== 阶段 6 · 跨进程持久化（关掉重开，记忆还在吗） ===")
    m.close()
    time.sleep(0.5)
    m2 = Mem(TEST_DATA)
    ok, st = stat(m2)
    n2 = num_field(st, "总节点数")
    rec("持久化", "重开进程后节点数不变", n2 == n1, f"关前 {n1} → 开后 {n2}")
    ok, titles, _ = recall_titles(m2, "渲染输出目录")
    hit = bool(titles) and overlap_ratio(FACTS[0][0], titles[0]) >= 0.5
    rec("持久化", "重开后「渲染输出目录」仍排第 1", ok and hit,
        f"top1={titles[0] if titles else '(空)'}")
    ok, titles, _ = recall_titles(m2, "错误处理")
    hit = bool(titles) and "anyhow" in titles[0]
    rec("持久化", "重开后「错误处理」仍排第 1", ok and hit,
        f"top1={titles[0] if titles else '(空)'}")
    m = m2

    # ── 阶段 7：累积到 20 条后，早期记忆是否被稀释 ──
    print(f"\n=== 阶段 7 · 累积（种子{seed_n} + {len(FACTS)} + {len(EXTRA)}）后的召回稳定性 ===")
    written = 0
    for content, dom in EXTRA:
        ok, t = write(m, content, dom)
        if ok and t.startswith("✅"):
            written += 1
        else:
            print(f"    ⚠️  未写入: {content!r} — {t[:60]}")
    rec("规模", f"EXTRA {len(EXTRA)} 条全部写入", written == len(EXTRA),
        f"实际写入 {written} 条")
    ok, st = stat(m)
    n3 = num_field(st, "总节点数")
    expect3 = seed_n + len(FACTS) + written
    rec("规模", f"节点数 = 种子+已写入 {expect3}", n3 == expect3, f"实际 {n3}")
    for content, _dom, kw, _syn in FACTS:
        ok, titles, _raw = recall_titles(m, kw)
        hit = bool(titles) and overlap_ratio(content, titles[0]) >= 0.5
        rec("规模·字面", f"「{kw}」仍排第 1", ok and hit,
            f"top1={titles[0] if titles else '(空)'}")

    m.close()

    # ── 阶段 8：真实库只读召回 ──
    print("\n=== 阶段 8 · 真实库（既有记忆）只读召回 ===")
    mr = Mem(REAL_DATA)
    ok, st = stat(mr)
    rec("真实库", "stats 可读", ok, (st.splitlines()[1] if len(st.splitlines()) > 1 else st)[:50])
    ok, titles, _ = recall_titles(mr, "渲染器用的是什么")
    hit = bool(titles) and "Cycles" in titles[0]
    rec("真实库", "「渲染器用的是什么」→ Cycles 排第 1", ok and hit,
        f"top1={titles[0] if titles else '(空)'}")
    ok, titles, _ = recall_titles(mr, "采样值设置多少")
    hit = bool(titles) and "128" in titles[0]
    rec("真实库", "「采样值设置多少」→ 128 排第 1", ok and hit,
        f"top1={titles[0] if titles else '(空)'}")
    ok, titles, _ = recall_titles(mr, "提交代码前要注意什么")
    hit = any("cargo fmt" in t for t in titles[:3])
    rec("真实库", "「提交前要注意什么」→ 命中 top3", ok and hit,
        f"top3={titles[:3] if titles else '(空)'}")
    mr.close()

    # ── 汇总 ──
    passed = sum(1 for *_x, ok, _d in RESULTS if ok)
    total = len(RESULTS)
    print(f"\n{'='*72}\n汇总：{passed}/{total} 通过\n{'='*72}")
    if passed != total:
        print("失败项：")
        for g, n, ok, d in RESULTS:
            if not ok:
                print(f"  ❌ [{g}] {n}  — {d}")

    if not args.keep and os.path.exists(TEST_DATA):
        shutil.rmtree(TEST_DATA)
        print(f"\n（已清理测试库 {TEST_DATA}；加 --keep 可保留）")
    return 0 if passed == total else 1


if __name__ == "__main__":
    sys.exit(main())
