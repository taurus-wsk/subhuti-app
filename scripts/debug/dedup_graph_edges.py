#!/usr/bin/env python3
"""清理 graph_edges 的两类历史脏数据（默认 dry-run，加 --apply 才写库）。

背景：
  `graph_edges` 主键是 (from_entity, to_entity, edge_kind)，带方向；而
  `EntityGraph` 是无向图。历史上有两个写入端问题：

  1. 端点未归一化 —— 同一对实体会以 (A,B) 和 (B,A) 各存一行，
     回灌时双向展开让内存边数成倍虚高，delete_edge 也只删得掉一半。
  2. kind 大小写不一致 —— 早期写入端用过小写 "manual"，反序列化
     的 fallback 分支会把它误判成可衰减的 Learned 边。

代码侧已修（存储层端点排序 + kind 兼容读取），但**已落库的脏行还在**，
本脚本负责把它们收敛成"无向 + 唯一"的规范形态。

用法：
    # 只看报告
    python3 scripts/debug/dedup_graph_edges.py

    # 真正执行清理（先自动备份）
    python3 scripts/debug/dedup_graph_edges.py --apply
"""
import argparse
import os
import shutil
import sqlite3
import sys
import time

DB = os.environ.get(
    "SUBHUTI_SUTRA_SQLITE",
    os.path.join(os.environ.get("SUBHUTI_DATA_DIR", os.path.expanduser("~/.subhuti/data")),
                 "sutra_library.sqlite"),
)

NORMALIZED_SELECT = """
SELECT MIN(from_entity, to_entity) AS from_entity,
       MAX(from_entity, to_entity) AS to_entity,
       CASE WHEN LOWER(edge_kind) = 'manual' THEN 'Manual' ELSE 'Learned' END AS edge_kind,
       MAX(weight) AS weight
FROM graph_edges
GROUP BY 1, 2, 3
"""


def report(con):
    total = con.execute("SELECT COUNT(*) FROM graph_edges").fetchone()[0]
    lower = con.execute(
        "SELECT COUNT(*) FROM graph_edges WHERE edge_kind <> 'Manual' AND edge_kind <> 'Learned'"
    ).fetchone()[0]
    kinds = con.execute(
        "SELECT edge_kind, COUNT(*) FROM graph_edges GROUP BY edge_kind ORDER BY 2 DESC"
    ).fetchall()
    # 归一化 + 去重后会剩多少行
    kept = con.execute(f"SELECT COUNT(*) FROM ({NORMALIZED_SELECT})").fetchone()[0]
    # 纯方向重复对数（kind 已归一时）
    dupdir = con.execute(
        "SELECT COUNT(*) FROM graph_edges a JOIN graph_edges b "
        "ON a.from_entity = b.to_entity AND a.to_entity = b.from_entity "
        "AND a.edge_kind = b.edge_kind AND a.from_entity < a.to_entity"
    ).fetchone()[0]
    print(f"库路径              : {DB}")
    print(f"graph_edges 当前行数: {total}")
    print(f"  kind 分布         : {kinds}")
    print(f"  非标准 kind 行数  : {lower}")
    print(f"  方向重复对数      : {dupdir}")
    print(f"清理后预计行数      : {kept}  (将删除 {total - kept} 行)")
    return total, kept


def apply(con):
    # 先把 WAL 合并进主库文件，否则 copy2 只复制主文件会漏掉未落盘的部分
    con.commit()
    con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    backup = f"{DB}.bak-graphdedup-{time.strftime('%H%M%S')}"
    shutil.copy2(DB, backup)
    print(f"\n已备份 -> {backup}")
    con.executescript(f"""
        BEGIN;
        CREATE TABLE graph_edges_dedup (
            from_entity TEXT NOT NULL,
            to_entity   TEXT NOT NULL,
            edge_kind   TEXT NOT NULL,
            weight      REAL NOT NULL DEFAULT 0.3,
            PRIMARY KEY (from_entity, to_entity, edge_kind)
        );
        INSERT INTO graph_edges_dedup (from_entity, to_entity, edge_kind, weight)
        {NORMALIZED_SELECT};
        DROP TABLE graph_edges;
        ALTER TABLE graph_edges_dedup RENAME TO graph_edges;
        COMMIT;
    """)
    print("清理完成")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="真正执行清理（默认只报告）")
    args = ap.parse_args()

    if not os.path.exists(DB):
        print(f"库不存在: {DB}", file=sys.stderr)
        return 1

    con = sqlite3.connect(DB)
    try:
        before, kept = report(con)
        if not args.apply:
            print("\n（dry-run）加 --apply 执行清理")
            return 0
        if before == kept:
            print("\n无需清理")
            return 0
        apply(con)
        report(con)
    finally:
        con.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
