#!/usr/bin/env python3
"""可观测性数据快照 —— 一条命令看清 trace / token / 记忆三块的真实落库状态。

用法：
    python3 scripts/debug/observability_snapshot.py
    SUBHUTI_DATA_DIR=/path/to/data python3 scripts/debug/observability_snapshot.py

只读打开 sqlite（?mode=ro），不会写入或改动任何数据。
"""

import os
import sqlite3
import sys

DATA_DIR = os.environ.get("SUBHUTI_DATA_DIR") or os.path.expanduser("~/.subhuti/data")
TRACES = os.path.join(DATA_DIR, "traces.sqlite")
SUTRA = os.path.join(DATA_DIR, "sutra_library.sqlite")


def ro(path):
    return sqlite3.connect(f"file:{path}?mode=ro", uri=True)


def line(title):
    print("\n" + "=" * 68)
    print(title)
    print("=" * 68)


def section_traces():
    if not os.path.exists(TRACES):
        print(f"  [跳过] traces 库不存在: {TRACES}")
        return
    c = ro(TRACES)

    line("① 链路存量")
    for tb in ("trace_spans", "trace_summaries", "trace_fn_calls", "trace_fn_logs"):
        try:
            print(f"  {tb:20s} {c.execute(f'SELECT COUNT(*) FROM {tb}').fetchone()[0]:>7}")
        except sqlite3.Error as e:
            print(f"  {tb:20s} 读取失败: {e}")

    line("② span 类型分布")
    for st, n, dur in c.execute(
        "SELECT span_type, COUNT(*), COALESCE(SUM(duration_ms),0) "
        "FROM trace_spans GROUP BY span_type ORDER BY COUNT(*) DESC"
    ):
        print(f"  {st:20s} {n:>5} 条   累计 {dur/1000:>8.1f}s")

    line("③ 成败")
    ok, bad = c.execute("SELECT SUM(success=1), SUM(success=0) FROM trace_spans").fetchone()
    print(f"  span  成功 {ok} / 失败 {bad}")
    for st, n in c.execute("SELECT status, COUNT(*) FROM trace_summaries GROUP BY status"):
        print(f"  trace {st:10s} {n}")

    line("④ token 采集覆盖（核心指标）")
    rows = list(
        c.execute(
            "SELECT tokens, duration_ms FROM trace_spans "
            "WHERE span_type='llm_responded'"
        )
    )
    total = len(rows)
    filled = [(t, d or 0) for t, d in rows if t and t > 0]
    empty = [(t, d or 0) for t, d in rows if not t or t <= 0]
    tk = sum(t for t, _ in filled)
    print(f"  llm_responded 总数 {total}  →  有 token {len(filled)} / 空 {len(empty)}"
          f"   填充率 {len(filled)/total*100:.0f}%" if total else "  无数据")
    print(f"  已采 token 合计 = {tk}")
    if filled:
        print(f"    [有 token] 调用 {len(filled):>3}  合计 {tk:>7}  平均耗时 "
              f"{sum(d for _, d in filled)/len(filled)/1000:.1f}s")
    if empty:
        print(f"    [空 token] 调用 {len(empty):>3}  合计 {'0':>7}  平均耗时 "
              f"{sum(d for _, d in empty)/len(empty)/1000:.1f}s")
        if filled:
            rate = tk / (sum(d for _, d in filled) / 1000.0)
            missing = rate * (sum(d for _, d in empty) / 1000.0)
            est = tk + missing
            print(f"\n  非流式速率 ≈ {rate:.1f} tok/s → 漏计估算 ≈ {missing:.0f} tokens")
            print(f"  真实量级 ≈ {est:.0f}  →  估算覆盖率 ≈ {tk/est*100:.0f}%")
            print("  ⚠️ 空 token 的调用均为流式路径（chat_streaming 无用量返回通道）")


def section_sutra():
    if not os.path.exists(SUTRA):
        print(f"\n  [跳过] 藏经阁库不存在: {SUTRA}")
        return
    c = ro(SUTRA)
    line("⑤ 藏经阁（记忆闭环）")
    for tb in [r[0] for r in c.execute(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
    )]:
        try:
            print(f"  {tb:24s} {c.execute(f'SELECT COUNT(*) FROM {tb}').fetchone()[0]:>7}")
        except sqlite3.Error as e:
            print(f"  {tb:24s} 读取失败: {e}")
    print("  集合明细:")
    for name, domain, n in c.execute(
        "SELECT name, domain, COUNT(*) FROM memory_collections GROUP BY name, domain"
    ):
        print(f"    {name} / {domain}  ×{n}")
    print("  ⚠️ memory_nodes=0 表示「运行中沉淀 → 下次召回」闭环未建立（retrieve 走静态内置知识库）")


def main():
    print(f"数据目录: {DATA_DIR}")
    if not os.path.exists(DATA_DIR):
        print("目录不存在。设置 SUBHUTI_DATA_DIR 指向实际数据目录后重试。")
        return 1
    section_traces()
    section_sutra()
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
