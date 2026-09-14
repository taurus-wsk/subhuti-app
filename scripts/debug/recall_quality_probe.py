#!/usr/bin/env python3
"""藏经阁召回质量回归探针：不只问"通不通"，而是问"排得对不对"。

与 `mcp_memory_probe.py` 的分工：
  - mcp_memory_probe  → 能力矩阵（工具是否暴露、write/recall/stats 是否可用）
  - 本脚本            → 排序质量（正确答案是否排在第 1、对照查询是否拒答）

判定标准（全部满足才算 PASS）：
  1. 每条有答案的查询，**正确答案必须出现在第 1 条**（而不是仅"出现在结果里"）；
  2. 对照查询（库里没有相关记忆）必须返回"未找到匹配的记忆"；
  3. 结果里不得出现 `(无标题)` 或 `总分: 0.0000` 这类零信息脏项。

用法：
    cargo build --bin subhuti
    SUBHUTI_BIN=target/debug/subhuti SUBHUTI_DATA_DIR=~/sqlite \\
      python3 scripts/debug/recall_quality_probe.py
"""
import json
import os
import re
import subprocess
import sys
import threading
import time

BIN = os.environ.get("SUBHUTI_BIN", "target/debug/subhuti")
DATA_DIR = os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/.subhuti/data"))

# (标签, 查询, 期望出现在第 1 条的线索, 是否为对照查询)
CASES = [
    ("命中/短事实", "渲染器用的是什么", "Cycles", False),
    ("命中/数值", "采样值设置多少", "128", False),
    ("命中/输出格式", "输出格式用什么", "PNG", False),
    ("命中/静态知识", "Rust 异步运行时用什么", "tokio", False),
    ("命中/序列化", "序列化通常用什么库", "serde", False),
    ("命中/提交规范", "提交前要跑什么", "cargo fmt", False),
    ("对照/不相关", "红烧肉怎么做最好吃", None, True),
]


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

    def close(self):
        try:
            self.proc.terminate()
            self.proc.wait(timeout=5)
        except Exception:
            try:
                self.proc.kill()
            except Exception:
                pass


def call(c, args, timeout=180):
    r = c.request("tools/call", {"name": "subhuti_memory", "arguments": args}, timeout=timeout)
    res = r.get("result")
    if res is None:
        return True, json.dumps(r.get("error") or {}, ensure_ascii=False)
    return bool(res.get("isError")), ((res.get("content") or [{}])[0].get("text", "") or "")


ENTRY_RE = re.compile(r"^\s*(\d+)\.\s+\*\*(.+?)\*\*", re.M)


def parse_entries(text):
    """把 recall 输出切成 [{rank, title, block}]，block 含标题行及其后缩进行。"""
    lines = text.splitlines()
    entries = []
    cur = None
    for ln in lines:
        m = ENTRY_RE.match(ln)
        if m:
            if cur:
                entries.append(cur)
            cur = {"rank": int(m.group(1)), "title": m.group(2).strip(), "block": ln}
        elif cur is not None:
            cur["block"] += "\n" + ln
    if cur:
        entries.append(cur)
    return entries


def main():
    print(f"二进制 : {BIN}")
    print(f"数据目录: {DATA_DIR}\n")

    failures = []
    c = Client()
    try:
        c.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                 "clientInfo": {"name": "rq-probe", "version": "1.0"}}, timeout=30)

        _, stats = call(c, {"action": "stats"})
        print("=" * 74)
        print("① 藏经阁统计")
        print("=" * 74)
        print(stats)

        print()
        print("=" * 74)
        print("② 召回质量")
        print("=" * 74)

        for label, q, expect, is_control in CASES:
            print(f"\n--[{label}] 查询: {q!r}" + (f"   期望第 1 条含: {expect}" if expect else "   期望: 拒答"))
            try:
                _, txt = call(c, {"action": "recall", "query": q, "top_k": 3})
            except TimeoutError as e:
                print(f"   ❌ 超时: {e}")
                failures.append(f"{label}: 超时")
                continue

            if is_control:
                ok = "未找到匹配的记忆" in txt
                print(f"   {'✅ 对照查询正确拒答' if ok else '❌ 对照查询应拒答却返回了结果'}")
                if not ok:
                    failures.append(f"{label}: 应拒答却返回结果")
                    print(txt)
                continue

            entries = parse_entries(txt)
            if not entries:
                print("   ❌ 无结果")
                failures.append(f"{label}: 无结果")
                continue

            top1 = entries[0]
            hit = expect in top1["block"]
            print(f"   第 1 条: {top1['title'][:56]!r}")
            print(f"   {'✅ PASS' if hit else '❌ FAIL'} 线索 {expect!r} {'命中' if hit else '未命中'}第 1 条")
            if not hit:
                failures.append(f"{label}: {expect!r} 不在第 1 条（实际第 1 条: {top1['title'][:40]}）")
                print(txt)

            # 脏项检查：零信息条目不得出现在结果里
            for e in entries:
                if "(无标题)" in e["title"]:
                    failures.append(f"{label}: 出现 (无标题) 脏项")
                if "总分: 0.0000" in e["block"]:
                    failures.append(f"{label}: 出现 0 分脏项")
    finally:
        c.close()

    print()
    print("=" * 74)
    if failures:
        print(f"❌ 召回质量未达标，{len(failures)} 项失败：")
        for f in failures:
            print(f"   - {f}")
        return 1
    print(f"✅ 召回质量全部达标（{len(CASES)} 条用例）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
