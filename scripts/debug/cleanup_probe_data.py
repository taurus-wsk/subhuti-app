#!/usr/bin/env python3
"""清理藏经阁里的探针/测试残留与「提问意图」垃圾记忆。

背景（2026-09-14 诊断）：
库中 18 条节点只有约 6 条是真记忆，其余是三类噪声——
  A 探针残留：`探针标记 A7F3`、`graphprobe…`、`WorkBuddy 记忆写入探针`、`eval-sticky`；
  B 提问意图垃圾：LLM 把"用户想知道什么"当成事实沉淀
    （`关注 Rust 模块在项目中的角色`、`需要了解…`、`希望获取…`、`对…有明确需求`）；
  C 同一批污染的整条会话：与 B 同 path（`/session/<id>#consolidate`）的兄弟节点
    （`项目涉及 Rust 模块职责介绍`、`错误处理需求`）也是话题描述，一并清掉。

第三轮（同日稍晚）又发现两类噪声，判据已同步进 `is_intent`：
  D **通用知识句**：助手回答里的通用知识被当成用户事实沉淀
    （`Rust 模块支持抽象和复用功能`、`Rust 模块用于组织代码和封装`）——
    它们与用户提问共享「Rust 模块」等 n-gram，能骗过 `grounded_in` 的字面重叠门槛；
  E **各测试脚本写死的 session_id**：`eval-sticky-1` / `edge-emoji` / `iso-N`
    / `probe-<ts>`，属纯测试残留，按路径特征清理。

判据与 Rust 侧 `MemoryConsolidator::is_intent_sentence` 保持同源，
`--self-test` 会校验两者一致。

同时清空 `graph_edges`：边的真相已改为「实体索引 + 当前建边规则」，
程序启动时会按新规则从 `graph_entity_chunks` 重建，库里那份可能由旧规则
（含大量 2 字碎片共现）生成的边不应保留。

用法：
    python3 scripts/debug/cleanup_probe_data.py --self-test  # 校验判据与 Rust 侧一致
    python3 scripts/debug/cleanup_probe_data.py            # 只报告，不改动
    python3 scripts/debug/cleanup_probe_data.py --apply    # 备份后执行
"""
import argparse
import os
import shutil
import sqlite3
import time

DATA_DIR = os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/sqlite"))
DB = os.environ.get("SUBHUTI_SUTRA_SQLITE", os.path.join(DATA_DIR, "sutra_library.sqlite"))

PROBE_COLLECTIONS = {"probe_memory"}

PROBE_TITLE_HINTS = (
    "探针标记",
    "调试标记",
    "graphprobe",
    "记忆写入探针",
    "eval-sticky",
)
# 注意：**不要**用 "probe-" 做路径匹配。`/session/sutra-probe-*` 下那 3 条
# （用户渲染器用 Cycles / 采样值固定 128 / 输出格式统一 PNG）虽然出自探针会话，
# 但内容是有意义的真实配置，且是召回质量探针的验证靶子，误删会让"召回是否
# 排对"无从验证。只按标题特征判定探针残留。
#
# 这些路径是各测试脚本**写死**的 session_id（不是推测），删掉它们等于清测试残留：
#   /session/eval-    → mcp_eval.py（eval-progress-*, eval-chatsem-*）
#                       与 mcp_eval_deep.py（eval-deep-*）
#   /session/edge-    → mcp_eval_deep.py 输入边界用例（edge-emoji, edge-long, edge-blank）
#   /session/iso-     → mcp_eval_deep.py 并发隔离用例（iso-1, iso-2 …）
#   /session/probe-   → mcp_memory_probe.py 写入探针（probe-<ts>, probe-dup-<ts>）
#   /session/envprobe-→ mcp_env_probe.py 环境自检（会真跑一次 chat 并沉淀）
#   eval-sticky       → mcp_eval.py 会话粘性用例
#
# ⚠️ 测试脚本调 subhuti_chat 时**必须显式传 session_id**。不传时服务端生成随机
# UUID，沉淀出的记忆带一个每次跑都变的路径 → 这里的路径特征完全抓不到，
# 实测因此漏过 4 条「Rust 语言」类残留。
PROBE_PATH_HINTS = (
    "graphprobe",
    "eval-sticky",
    "/session/eval-",
    "/session/edge-",
    "/session/iso-",
    "/session/probe-",
    "/session/envprobe-",
    "/session/mcpverify-",
)

# 以下六组判据表必须与 Rust 侧 `MemoryConsolidator::is_intent_sentence`
# （src/application/memory_consolidation.rs）逐项保持一致，
# 否则脚本会漏判/误判落库节点。改一处请同步改另一处；
# `--self-test` 会把两端用同一批样本钉住，漂移即报警。
INTENT_PREFIXES = (
    "关注", "需要", "希望", "想要", "想了解", "想知道",
    "询问", "请问", "求助", "帮忙", "如何", "怎么", "为什么", "是否", "能否", "求",
    # 动作片段引导词："了解几何修改器" / "探索修改器类型"
    "了解", "探索", "学习", "研究", "查看",
)
INTENT_PHRASES = (
    "需要了解", "希望了解", "希望获取", "希望知道", "想了解", "想知道",
    "有明确需求", "明确需求", "提了个问题", "提出疑问", "询问", "寻求",
)
# ③ 第三人称复述：以「用户/我」开头 + 泛意图动词
USER_INTENT_VERBS = (
    "关注", "了解", "提及", "提到", "询问", "想知道",
    "需要查看", "需要了解", "需要获取",
    "有具体需求", "有明确需求",
    "希望了解", "希望知道", "希望获取", "希望查看",
)
# ④ 话题标题式：名词短语 + 这些尾巴（仅对 ≤25 字的短句判定，避免误伤长正文）
TOPIC_TAILS = ("介绍", "描述", "说明", "应用", "作用", "概述")
TOPIC_TAIL_MAX_CHARS = 25
# ④ 段豁免词：句中出现这些 → 是陈述句而非话题标题。
#   实测误杀样本「提交信息用中文描述」（含「用」）。宁可漏放一句废话，
#   不可误杀真事实 —— 前者事后可清，后者是记忆丢失。
TOPIC_TAIL_EXEMPT = ("用", "是", "为", "必须")
# ⑤ 通用知识句：无用户锚点 + 能力描述谓语
#    「Rust 模块支持抽象和复用功能」——助手回答里的通用知识，不是用户事实
# ⚠️ 锚点里**不能**出现裸「项目」：「项目涉及 Rust 编程」正是靠它逃逸的
#    （2026-09-14 实测落库）。只认「本项目」这种明确指代。
ANCHORS = (
    "用户", "我", "本项目", "我们", "偏好", "默认", "固定", "配置", "约定",
    "记住", "注意",
)
GENERIC_PREDICATES = (
    "支持", "用于", "包括", "分为", "可分为", "属于", "指的是", "是指", "之分", "组成",
    "涉及",
)


def is_probe(title, path, coll_name):
    if coll_name in PROBE_COLLECTIONS:
        return "collection=probe_memory"
    for h in PROBE_TITLE_HINTS:
        if h in (title or ""):
            return f"标题含「{h}」"
    for h in PROBE_PATH_HINTS:
        if h in (path or ""):
            return f"路径含「{h}」"
    return None


def is_intent(title, content):
    """六段判定，镜像 Rust 侧 `is_intent_sentence`。

    判定对象与 Rust 一致：优先用标题，标题为空才退回正文。
    """
    text = (title or "").strip() or (content or "").strip()
    if not text:
        return None

    # ① 句首意图动词
    for p in INTENT_PREFIXES:
        if text.startswith(p):
            return f"意图句前缀「{p}」"

    # ② 句中意图组合词
    for p in INTENT_PHRASES:
        if p in text:
            return f"意图句「{p}」"

    # ③ 「用户/我 + 泛意图动词」第三人称复述
    if text.startswith("用户") or text.startswith("我"):
        for v in USER_INTENT_VERBS:
            if v in text:
                return f"第三人称复述「{v}」"

    # ④ 话题标题式（短句才判定）；豁免词**只看尾巴之前**的部分 ——
    #    尾巴「应用」「作用」自身就含「用」，对整句做 in 判断会漏判这两类。
    if len(text) <= TOPIC_TAIL_MAX_CHARS:
        for t in TOPIC_TAILS:
            if not text.endswith(t):
                continue
            stem = text[: len(text) - len(t)]
            if any(w in stem for w in TOPIC_TAIL_EXEMPT):
                continue
            return f"话题标题式「{t}」"

    # ⑤ 通用知识句：无用户锚点 + 能力描述谓语（短句才判定）
    if len(text) <= TOPIC_TAIL_MAX_CHARS:
        if not any(a in text for a in ANCHORS):
            for p in GENERIC_PREDICATES:
                if p in text:
                    return f"通用知识句「{p}」"

    return None


def load_nodes(cur):
    cur.execute("""
        SELECT n.node_id, n.title, n.path, n.content, n.collection_id, c.name
        FROM memory_nodes n LEFT JOIN memory_collections c ON c.collection_id = n.collection_id
    """)
    return cur.fetchall()


def plan(cur):
    """算出要删的节点，返回 {node_id: reason}。"""
    nodes = load_nodes(cur)
    doomed = {}

    # A 探针残留
    for node_id, title, path, _content, _cid, coll in nodes:
        reason = is_probe(title, path, coll)
        if reason:
            doomed[node_id] = reason

    # B 提问意图 + C 同 path 整批
    dirty_paths = set()
    for node_id, title, path, content, _cid, _coll in nodes:
        if node_id in doomed:
            continue
        reason = is_intent(title, content)
        if reason:
            doomed[node_id] = reason
            if path:
                dirty_paths.add(path)

    for node_id, title, path, _content, _cid, _coll in nodes:
        if node_id in doomed:
            continue
        if path and path in dirty_paths:
            doomed[node_id] = f"同批污染会话（{path}）"

    return doomed


# 与 Rust 侧 memory_consolidation.rs 的测试同源。
# 两端判据漂移时，`--self-test` 会立刻报警。
JUNK_CASES = (
    # 第一轮实测落库（句首 / 句中意图词）
    "关注 Rust 模块在项目中的角色",
    "需要了解 Rust 模块的具体职责",
    "希望获取 Rust 模块职责的详细信息",
    "对 Rust 模块职责有明确需求",
    "我希望了解 rust 模块的职责",
    # 第二轮实测落库（第三人称复述 / 话题标题式）
    "用户关注 Rust 模块职责",
    "用户了解 Rust 编程",
    "用户提及项目代码和模块",
    "用户需要查看 Blender 动画",
    "用户对 Blender 动画有具体需求",
    "Blender 修改器介绍",
    "几何修改器功能描述",
    "网格修改器类型说明",
    "布尔运算在Blender中的应用",
    "细分算法在Blender中作用",
    # 第三轮实测落库（通用知识句 / 动作片段）
    "Rust 模块支持抽象和复用功能",
    "Rust 模块支持模块化设计",
    "Rust 模块有顶层、嵌套和私有之分",
    "Rust 模块用于组织代码和封装",
    "了解几何修改器",
    "探索修改器类型",
    # 第四轮实测落库（泛话题 + 弱谓语「涉及」；锚点表曾含裸「项目」，被它逃逸）
    "项目涉及 Rust 编程",
)
REAL_FACT_CASES = (
    "用户渲染器用 Cycles",
    "采样值固定 128",
    "用户偏好：提交前必须跑 cargo fmt 与 cargo clippy",
    "输出格式统一 PNG",
    "用户希望默认走 20 积分路径，不加 PBR",
    "Rust 基础知识与最佳实践",
    "Rust 设计模式",
    "个人编码约定",
    "项目上下文",
    # 带用户锚点的能力描述是真事实，不能被 ⑤ 段误杀
    "本项目支持多租户",
    "用户项目支持插件扩展",
    # 「涉及」+ 锚点同样是真事实（⑤ 段只打无锚点的）
    "用户的项目涉及支付与风控",
    "本项目涉及 Blender 与 Rust 两端",
    # ④ 段豁免：以话题尾收尾但含动作词 → 陈述句而非话题标题
    "提交信息用中文描述",
)


def self_test():
    """校验脚本判据与 Rust 侧一致。返回 0/1。"""
    bad = []
    for junk in JUNK_CASES:
        if not is_intent(junk, ""):
            bad.append(f"应判为垃圾但漏判: {junk!r}")
    for fact in REAL_FACT_CASES:
        if is_intent(fact, ""):
            bad.append(f"应保留但被误判: {fact!r} → {is_intent(fact, '')}")

    if bad:
        print("❌ 判据自检失败：")
        for b in bad:
            print(f"   - {b}")
        return 1

    print(f"✅ 判据自检通过：{len(JUNK_CASES)} 条垃圾全部识别、"
          f"{len(REAL_FACT_CASES)} 条真实事实全部保留")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="备份后执行删除")
    ap.add_argument("--self-test", action="store_true", help="只校验判据是否与 Rust 侧一致")
    args = ap.parse_args()

    if args.self_test:
        return self_test()

    if not os.path.exists(DB):
        print(f"❌ 库不存在: {DB}")
        return 1

    con = sqlite3.connect(DB)
    con.execute("PRAGMA journal_mode=WAL")
    cur = con.cursor()

    total = cur.execute("SELECT COUNT(*) FROM memory_nodes").fetchone()[0]
    doomed = plan(cur)

    print(f"库: {DB}")
    print(f"节点总数: {total}   待清理: {len(doomed)}   保留: {total - len(doomed)}\n")

    print("=== 待清理节点 ===")
    for node_id, reason in doomed.items():
        row = cur.execute(
            "SELECT title, path FROM memory_nodes WHERE node_id = ?", (node_id,)
        ).fetchone()
        title, path = row if row else ("?", "?")
        print(f"  [{reason}]")
        print(f"      {title!r}")
        print(f"      {path}")
    if not doomed:
        print("  （无）")

    edges_before = cur.execute("SELECT COUNT(*) FROM graph_edges").fetchone()[0]
    chunks_before = cur.execute("SELECT COUNT(*) FROM graph_entity_chunks").fetchone()[0]
    print("\n=== 图谱 ===")
    print(f"  graph_edges: {edges_before} 行 → 清空（启动时按新规则重建）")
    print(f"  graph_entity_chunks: {chunks_before} 行 → 保留（实体索引是边的真相来源）")

    if not args.apply:
        print("\n（预演模式，未改动。加 --apply 执行）")
        con.close()
        return 0

    suffix = time.strftime("%y%m%d-%H%M%S")
    backup = f"{DB}.bak-cleanup-{suffix}"
    bak_con = sqlite3.connect(backup)
    con.backup(bak_con)
    bak_con.close()
    print(f"\n✅ 已备份: {backup}")

    ids = list(doomed.keys())
    ph = ",".join("?" * len(ids))
    if ids:
        cur.execute(f"DELETE FROM memory_nodes WHERE node_id IN ({ph})", ids)
        # `graph_entity_chunks` 的 chunk_id 就是 node_id，entity_id 是实体名。
        # 只删与被删节点相关的行；存留节点的索引必须原样保留。
        affected = cur.execute(
            f"SELECT COUNT(*) FROM graph_entity_chunks "
            f"WHERE chunk_id IN ({ph}) OR entity_id IN ({ph})",
            ids + ids,
        ).fetchone()[0]
        cur.execute(
            f"DELETE FROM graph_entity_chunks "
            f"WHERE chunk_id IN ({ph}) OR entity_id IN ({ph})",
            ids + ids,
        )
        print(f"✅ 已删除 {affected} 行图谱实体索引关联（对应 {len(ids)} 个节点）")

    cur.execute("DELETE FROM graph_edges")
    print(f"✅ 已清空 graph_edges（{edges_before} 行）")

    con.commit()
    cur.execute("PRAGMA wal_checkpoint(TRUNCATE)")

    remain = cur.execute("SELECT COUNT(*) FROM memory_nodes").fetchone()[0]
    chunks_after = cur.execute("SELECT COUNT(*) FROM graph_entity_chunks").fetchone()[0]
    print(f"\n📊 清理后节点数: {remain}   graph_entity_chunks 残留: {chunks_after}")
    con.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
