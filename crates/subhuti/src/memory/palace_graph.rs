//! # 记忆宫殿图 (Memory Palace Graph)
//!
//! 空间记忆组织图，将记忆按"宫殿"隐喻进行结构化组织，提供可导航的联想记忆结构。
//!
//! ## 宫殿隐喻 (Palace Metaphor)
//!
//! - **Wing (翼/领域)**: 宫殿的一个侧翼，代表一个领域/类别 (domain/category)。
//!   每个 Wing 包含多个 Room。
//! - **Room (房间/主题)**: Wing 内的一个房间，代表一个主题 (topic)。
//!   每个 Room 存放该主题下的记忆（抽屉 drawer）。
//! - **Hallway (走廊)**: Wing 内部实体间的连接，基于共现关系。
//!   同一领域内的实体若在记忆中共同出现，则通过走廊相连。
//! - **Tunnel (通道)**: 跨 Wing 的 Room 间连接，形成跨领域的联想通路。
//!   通过通道可以在不同领域间导航。

use super::dynamics::ConnectionDynamics;
use super::MemoryItem;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};
use uuid::Uuid;

// ── Wing (领域) ──────────────────────────────────────

/// 翼/领域 - 宫殿中的一个侧翼，代表一个领域/类别
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wing {
    /// 领域名称
    pub name: String,
    /// 领域描述
    pub description: String,
    /// 该领域下的房间数
    pub room_count: usize,
}

// ── Room (主题) ──────────────────────────────────────

/// 房间/主题 - Wing 内的一个房间，代表一个主题
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    /// 主题名称
    pub name: String,
    /// 所属领域
    pub wing: String,
    /// 该房间内的记忆数量 (drawer = 抽屉)
    pub drawer_count: usize,
}

// ── Hallway (走廊) ──────────────────────────────────

/// 走廊 - Wing 内部实体间的连接，基于共现关系
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hallway {
    /// 唯一 ID
    pub id: String,
    /// 所属领域
    pub wing: String,
    /// 实体 A
    pub entity_a: String,
    /// 实体 B
    pub entity_b: String,
    /// 共现次数
    pub co_occurrence_count: usize,
    /// 连接动力学状态
    pub dynamics: ConnectionDynamics,
}

// ── Tunnel (通道) ────────────────────────────────────

/// 通道 - 跨 Wing 的 Room 间连接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tunnel {
    /// 唯一 ID
    pub id: String,
    /// 源领域
    pub source_wing: String,
    /// 源房间
    pub source_room: String,
    /// 目标领域
    pub target_wing: String,
    /// 目标房间
    pub target_room: String,
    /// 通道标签（描述连接关系）
    pub label: String,
    /// 连接动力学状态
    pub dynamics: ConnectionDynamics,
}

// ── 统计 ─────────────────────────────────────────────

/// 记忆宫殿图统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PalaceGraphStats {
    /// 领域数
    pub wing_count: usize,
    /// 房间数
    pub room_count: usize,
    /// 走廊数
    pub hallway_count: usize,
    /// 通道数
    pub tunnel_count: usize,
}

// ── PalaceGraph 主结构 ───────────────────────────────

/// 记忆宫殿图 - 空间记忆组织图管理器（内存态）
///
/// 维护 Wing -> Room -> Hallway/Tunnel 的层级结构，
/// 提供可导航的联想记忆结构。
pub struct PalaceGraph {
    /// 领域表：name -> Wing
    pub wings: HashMap<String, Wing>,
    /// 房间表：name -> Room
    pub rooms: HashMap<String, Room>,
    /// 走廊列表（Wing 内部实体连接）
    pub hallways: Vec<Hallway>,
    /// 通道列表（跨 Wing 房间连接）
    pub tunnels: Vec<Tunnel>,
}

impl Default for PalaceGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl PalaceGraph {
    /// 创建空的记忆宫殿图
    pub fn new() -> Self {
        Self {
            wings: HashMap::new(),
            rooms: HashMap::new(),
            hallways: Vec::new(),
            tunnels: Vec::new(),
        }
    }

    /// 添加领域 (Wing)
    ///
    /// 若同名领域已存在则更新描述，room_count 保持不变。
    pub fn add_wing(&mut self, name: &str, description: &str) {
        let exists = self.wings.contains_key(name);
        let entry = self.wings.entry(name.to_string()).or_insert(Wing {
            name: name.to_string(),
            description: description.to_string(),
            room_count: 0,
        });
        entry.description = description.to_string();
        if exists {
            debug!(
                "PalaceGraph: add_wing 更新领域 name={}, description_len={}",
                name,
                description.len()
            );
        } else {
            info!(
                "PalaceGraph: add_wing 添加新领域 name={}, description_len={}",
                name,
                description.len()
            );
        }
    }

    /// 添加房间 (Room) 到指定领域
    ///
    /// 若领域不存在则自动创建。若同名房间已存在则不重复添加。
    pub fn add_room(&mut self, name: &str, wing: &str) {
        info!("PalaceGraph: add_room name={}, wing={}", name, wing);
        if !self.wings.contains_key(wing) {
            debug!("PalaceGraph: add_room 领域不存在，自动创建 wing={}", wing);
            self.add_wing(wing, "");
        }
        if self.rooms.contains_key(name) {
            debug!("PalaceGraph: add_room 房间已存在，跳过 name={}", name);
            return;
        }
        self.rooms.insert(
            name.to_string(),
            Room {
                name: name.to_string(),
                wing: wing.to_string(),
                drawer_count: 0,
            },
        );
        if let Some(w) = self.wings.get_mut(wing) {
            w.room_count += 1;
        }
        debug!(
            "PalaceGraph: add_room 完成，wing={}, room_count={}",
            wing,
            self.wings.get(wing).map(|w| w.room_count).unwrap_or(0)
        );
    }

    /// 添加走廊 (Hallway) - Wing 内部实体间连接
    ///
    /// 若同一 Wing 内相同实体对（无序匹配）的走廊已存在，则递增 co_occurrence_count
    /// 并触发赫布式增强；否则创建新走廊。
    pub fn add_hallway(&mut self, wing: &str, entity_a: &str, entity_b: &str) {
        info!(
            "PalaceGraph: add_hallway wing={}, entity_a={}, entity_b={}",
            wing, entity_a, entity_b
        );
        let now = Utc::now();
        for h in self.hallways.iter_mut() {
            if h.wing == wing
                && ((h.entity_a == entity_a && h.entity_b == entity_b)
                    || (h.entity_a == entity_b && h.entity_b == entity_a))
            {
                h.co_occurrence_count += 1;
                h.dynamics.potentiate(now);
                debug!(
                    "PalaceGraph: add_hallway 递增已有走廊 co_occurrence_count={}, access_count={}",
                    h.co_occurrence_count, h.dynamics.access_count
                );
                return;
            }
        }
        let mut dynamics = ConnectionDynamics::new();
        dynamics.potentiate(now);
        let id = Uuid::new_v4().to_string();
        self.hallways.push(Hallway {
            id: id.clone(),
            wing: wing.to_string(),
            entity_a: entity_a.to_string(),
            entity_b: entity_b.to_string(),
            co_occurrence_count: 1,
            dynamics,
        });
        debug!("PalaceGraph: add_hallway 创建新走廊 id={}", id);
    }

    /// 添加通道 (Tunnel) - 跨 Wing 的 Room 间连接
    ///
    /// 返回新通道的 ID。
    pub fn add_tunnel(
        &mut self,
        source_wing: &str,
        source_room: &str,
        target_wing: &str,
        target_room: &str,
        label: &str,
    ) -> String {
        info!(
            "PalaceGraph: add_tunnel source={}:{}, target={}:{}, label={}",
            source_wing, source_room, target_wing, target_room, label
        );
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let mut dynamics = ConnectionDynamics::new();
        dynamics.potentiate(now);
        self.tunnels.push(Tunnel {
            id: id.clone(),
            source_wing: source_wing.to_string(),
            source_room: source_room.to_string(),
            target_wing: target_wing.to_string(),
            target_room: target_room.to_string(),
            label: label.to_string(),
            dynamics,
        });
        debug!("PalaceGraph: add_tunnel 创建新通道 id={}", id);
        id
    }

    /// 从起始房间通过通道进行 BFS 遍历
    ///
    /// 返回所有可达房间名（包含起始房间），最多跳数 `max_hops`。
    /// 通道是双向的（可从 source 到 target，也可反向）。
    pub fn traverse(&self, start_room: &str, max_hops: usize) -> Vec<String> {
        info!(
            "PalaceGraph: traverse 开始 start_room={}, max_hops={}, tunnel_count={}",
            start_room,
            max_hops,
            self.tunnels.len()
        );
        let mut visited: Vec<String> = vec![start_room.to_string()];
        let mut visited_set: HashMap<String, ()> = HashMap::new();
        visited_set.insert(start_room.to_string(), ());
        let mut frontier: Vec<String> = vec![start_room.to_string()];

        for hop in 0..max_hops {
            if frontier.is_empty() {
                debug!(
                    "PalaceGraph: traverse 第 {} 跳 frontier 为空，提前结束",
                    hop + 1
                );
                break;
            }
            let mut next_frontier: Vec<String> = Vec::new();
            for room in frontier.iter() {
                for t in self.tunnels.iter() {
                    let neighbor = if t.source_room == room.as_str() {
                        Some(&t.target_room)
                    } else if t.target_room == room.as_str() {
                        Some(&t.source_room)
                    } else {
                        None
                    };
                    if let Some(n) = neighbor {
                        if !visited_set.contains_key(n.as_str()) {
                            visited_set.insert(n.clone(), ());
                            visited.push(n.clone());
                            next_frontier.push(n.clone());
                        }
                    }
                }
            }
            debug!(
                "PalaceGraph: traverse 第 {} 跳完成，新增 {} 个可达房间",
                hop + 1,
                next_frontier.len()
            );
            frontier = next_frontier;
        }
        info!(
            "PalaceGraph: traverse 完成，共发现 {} 个可达房间",
            visited.len()
        );
        visited
    }

    /// 查找匹配条件的通道
    ///
    /// - `wing_a` / `wing_b` 为 None 表示不限制该侧领域。
    /// - 同时提供时，匹配 `source_wing == wing_a && target_wing == wing_b`
    ///   （顺序敏感，便于表达方向性查询）。
    pub fn find_tunnels(&self, wing_a: Option<&str>, wing_b: Option<&str>) -> Vec<&Tunnel> {
        info!(
            "PalaceGraph: find_tunnels wing_a={:?}, wing_b={:?}",
            wing_a, wing_b
        );
        let results: Vec<&Tunnel> = self
            .tunnels
            .iter()
            .filter(|t| {
                if let Some(a) = wing_a {
                    if t.source_wing != a {
                        return false;
                    }
                }
                if let Some(b) = wing_b {
                    if t.target_wing != b {
                        return false;
                    }
                }
                true
            })
            .collect();
        debug!(
            "PalaceGraph: find_tunnels 完成，找到 {} 条通道",
            results.len()
        );
        results
    }

    /// 列出走廊，可按领域过滤
    pub fn list_hallways(&self, wing: Option<&str>) -> Vec<&Hallway> {
        info!("PalaceGraph: list_hallways wing={:?}", wing);
        let results: Vec<&Hallway> = self
            .hallways
            .iter()
            .filter(|h| wing.is_none_or(|w| h.wing == w))
            .collect();
        debug!(
            "PalaceGraph: list_hallways 完成，找到 {} 条走廊",
            results.len()
        );
        results
    }

    /// 从记忆列表计算指定领域内的走廊
    ///
    /// 简单策略：从每条记忆内容中按空白分词，过滤长度 > 3 的 token 作为"实体"，
    /// 统计同一领域内 token 对的共现次数，共现 >= 2 次则创建走廊。
    ///
    /// 返回计算得到的新走廊列表（不会自动加入图，调用方可自行 `add_hallway`）。
    pub fn compute_hallways_for_wing(&self, wing: &str, memories: &[MemoryItem]) -> Vec<Hallway> {
        info!(
            "PalaceGraph: compute_hallways_for_wing 开始 wing={}, memories={}",
            wing,
            memories.len()
        );
        let mut cooccur: HashMap<(String, String), usize> = HashMap::new();

        for (i, mem) in memories.iter().enumerate() {
            let mut tokens: Vec<String> = mem
                .content
                .split_whitespace()
                .map(|s| s.to_lowercase())
                .filter(|s| s.len() > 3)
                .collect();
            tokens.sort();
            tokens.dedup();

            for j in 0..tokens.len() {
                for k in (j + 1)..tokens.len() {
                    let key = (tokens[j].clone(), tokens[k].clone());
                    *cooccur.entry(key).or_insert(0) += 1;
                }
            }
            debug!(
                "PalaceGraph: compute_hallways_for_wing 处理第 {} 条记忆，token数={}",
                i + 1,
                tokens.len()
            );
        }

        debug!(
            "PalaceGraph: compute_hallways_for_wing 共现统计完成，候选对数={}",
            cooccur.len()
        );

        let now = Utc::now();
        let results: Vec<Hallway> = cooccur
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .map(|((a, b), count)| {
                let mut dynamics = ConnectionDynamics::new();
                dynamics.potentiate(now);
                Hallway {
                    id: Uuid::new_v4().to_string(),
                    wing: wing.to_string(),
                    entity_a: a,
                    entity_b: b,
                    co_occurrence_count: count,
                    dynamics,
                }
            })
            .collect();

        info!(
            "PalaceGraph: compute_hallways_for_wing 完成，生成 {} 条走廊",
            results.len()
        );
        results
    }

    /// 获取统计信息
    pub fn stats(&self) -> PalaceGraphStats {
        info!("PalaceGraph: stats 请求");
        let stats = PalaceGraphStats {
            wing_count: self.wings.len(),
            room_count: self.rooms.len(),
            hallway_count: self.hallways.len(),
            tunnel_count: self.tunnels.len(),
        };
        debug!(
            "PalaceGraph: stats 完成 wings={}, rooms={}, hallways={}, tunnels={}",
            stats.wing_count, stats.room_count, stats.hallway_count, stats.tunnel_count
        );
        stats
    }

    /// 删除指定 ID 的走廊
    ///
    /// 返回是否删除成功。
    pub fn delete_hallway(&mut self, id: &str) -> bool {
        info!("PalaceGraph: delete_hallway id={}", id);
        let before = self.hallways.len();
        self.hallways.retain(|h| h.id != id);
        let deleted = self.hallways.len() < before;
        debug!(
            "PalaceGraph: delete_hallway 完成，删除={}, 剩余={}",
            deleted,
            self.hallways.len()
        );
        deleted
    }

    /// 删除指定 ID 的通道
    ///
    /// 返回是否删除成功。
    pub fn delete_tunnel(&mut self, id: &str) -> bool {
        info!("PalaceGraph: delete_tunnel id={}", id);
        let before = self.tunnels.len();
        self.tunnels.retain(|t| t.id != id);
        let deleted = self.tunnels.len() < before;
        debug!(
            "PalaceGraph: delete_tunnel 完成，删除={}, 剩余={}",
            deleted,
            self.tunnels.len()
        );
        deleted
    }
}

// ── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryLayer;

    /// 辅助：创建一条 MemoryItem
    fn make_mem(content: &str) -> MemoryItem {
        MemoryItem::new(content.to_string(), MemoryLayer::Archive, None)
    }

    #[test]
    fn test_add_wing_and_room() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("tech", "技术领域");
        graph.add_room("rust", "tech");
        graph.add_room("python", "tech");

        let stats = graph.stats();
        assert_eq!(stats.wing_count, 1);
        assert_eq!(stats.room_count, 2);

        // 领域 room_count 应递增
        let wing = graph.wings.get("tech").unwrap();
        assert_eq!(wing.room_count, 2);
        assert_eq!(wing.description, "技术领域");
    }

    #[test]
    fn test_add_wing_updates_description() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("w", "old");
        graph.add_room("r", "w");
        graph.add_wing("w", "new"); // 更新描述，不应重置 room_count

        let wing = graph.wings.get("w").unwrap();
        assert_eq!(wing.description, "new");
        assert_eq!(wing.room_count, 1, "room_count should be preserved");
    }

    #[test]
    fn test_add_room_creates_wing_if_missing() {
        let mut graph = PalaceGraph::new();
        // 领域不存在时 add_room 应自动创建领域
        graph.add_room("room1", "auto_wing");
        assert!(graph.wings.contains_key("auto_wing"));
        assert_eq!(graph.wings.get("auto_wing").unwrap().room_count, 1);
    }

    #[test]
    fn test_add_room_duplicate_no_double_count() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("w", "");
        graph.add_room("r", "w");
        graph.add_room("r", "w"); // 重复添加
        assert_eq!(graph.rooms.len(), 1);
        assert_eq!(graph.wings.get("w").unwrap().room_count, 1);
    }

    #[test]
    fn test_add_hallway_new_and_increment() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("tech", "");

        // 新走廊
        graph.add_hallway("tech", "rust", "tokio");
        assert_eq!(graph.hallways.len(), 1);
        assert_eq!(graph.hallways[0].co_occurrence_count, 1);

        // 相同实体对（顺序一致）应递增
        graph.add_hallway("tech", "rust", "tokio");
        assert_eq!(graph.hallways.len(), 1);
        assert_eq!(graph.hallways[0].co_occurrence_count, 2);

        // 反序也应匹配同一走廊（实体对无序）
        graph.add_hallway("tech", "tokio", "rust");
        assert_eq!(graph.hallways.len(), 1);
        assert_eq!(graph.hallways[0].co_occurrence_count, 3);

        // 动力学应被增强（access_count 递增）
        assert!(graph.hallways[0].dynamics.access_count >= 3);
    }

    #[test]
    fn test_add_hallway_different_wings_separate() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("w1", "");
        graph.add_wing("w2", "");

        graph.add_hallway("w1", "a", "b");
        graph.add_hallway("w2", "a", "b");
        assert_eq!(graph.hallways.len(), 2);
    }

    #[test]
    fn test_add_tunnel() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("tech", "");
        graph.add_wing("science", "");
        graph.add_room("rust", "tech");
        graph.add_room("physics", "science");

        let id = graph.add_tunnel("tech", "rust", "science", "physics", "related");
        assert_eq!(graph.tunnels.len(), 1);
        assert_eq!(graph.tunnels[0].id, id);
        assert_eq!(graph.tunnels[0].label, "related");
        assert!(graph.tunnels[0].dynamics.access_count >= 1);
    }

    #[test]
    fn test_traverse_bfs_chain() {
        let mut graph = PalaceGraph::new();
        // 构造图：A -- B -- C -- D （链式）
        graph.add_tunnel("w", "A", "w", "B", "l1");
        graph.add_tunnel("w", "B", "w", "C", "l2");
        graph.add_tunnel("w", "C", "w", "D", "l3");

        // max_hops=0：只有起点
        let r0 = graph.traverse("A", 0);
        assert_eq!(r0, vec!["A".to_string()]);

        // max_hops=1：A -> B
        let r1 = graph.traverse("A", 1);
        assert!(r1.contains(&"A".to_string()));
        assert!(r1.contains(&"B".to_string()));
        assert!(!r1.contains(&"C".to_string()));

        // max_hops=3：可达 D（A->B->C->D）
        let r3 = graph.traverse("A", 3);
        assert!(r3.contains(&"D".to_string()));
        assert_eq!(r3.len(), 4);

        // 反向也可遍历（通道双向）
        let r_back = graph.traverse("D", 2);
        assert!(r_back.contains(&"B".to_string()));
    }

    #[test]
    fn test_traverse_cycle_no_repeat() {
        let mut graph = PalaceGraph::new();
        // 环：A -- B -- C -- A
        graph.add_tunnel("w", "A", "w", "B", "");
        graph.add_tunnel("w", "B", "w", "C", "");
        graph.add_tunnel("w", "C", "w", "A", "");

        let r = graph.traverse("A", 10);
        // 应只访问 3 个节点（不重复）
        assert_eq!(r.len(), 3);
        assert!(r.contains(&"A".to_string()));
        assert!(r.contains(&"B".to_string()));
        assert!(r.contains(&"C".to_string()));
    }

    #[test]
    fn test_traverse_unknown_room() {
        let graph = PalaceGraph::new();
        // 起点不在任何通道中，应只返回起点
        let r = graph.traverse("unknown", 5);
        assert_eq!(r, vec!["unknown".to_string()]);
    }

    #[test]
    fn test_find_tunnels() {
        let mut graph = PalaceGraph::new();
        graph.add_tunnel("tech", "rust", "science", "physics", "t1");
        graph.add_tunnel("tech", "rust", "math", "algebra", "t2");
        graph.add_tunnel("tech", "python", "science", "chem", "t3");

        // 无过滤
        assert_eq!(graph.find_tunnels(None, None).len(), 3);

        // 按 source_wing 过滤
        assert_eq!(graph.find_tunnels(Some("tech"), None).len(), 3);

        // 按 target_wing 过滤
        assert_eq!(graph.find_tunnels(None, Some("science")).len(), 2);

        // 双向过滤（顺序敏感）
        assert_eq!(graph.find_tunnels(Some("tech"), Some("science")).len(), 2);
        assert_eq!(graph.find_tunnels(Some("tech"), Some("math")).len(), 1);
        assert_eq!(graph.find_tunnels(Some("math"), Some("tech")).len(), 0);
    }

    #[test]
    fn test_list_hallways() {
        let mut graph = PalaceGraph::new();
        graph.add_hallway("w1", "a", "b");
        graph.add_hallway("w1", "c", "d");
        graph.add_hallway("w2", "e", "f");

        assert_eq!(graph.list_hallways(None).len(), 3);
        assert_eq!(graph.list_hallways(Some("w1")).len(), 2);
        assert_eq!(graph.list_hallways(Some("w2")).len(), 1);
        assert_eq!(graph.list_hallways(Some("w3")).len(), 0);
    }

    #[test]
    fn test_compute_hallways_for_wing() {
        let graph = PalaceGraph::new();
        // 构造记忆：
        //  mem1: "rust tokio framework"  → pairs: (framework,rust),(framework,tokio),(rust,tokio)
        //  mem2: "rust async runtime"    → pairs: (async,runtime),(async,rust),(runtime,rust)
        //  mem3: "rust tokio async"      → pairs: (async,rust),(async,tokio),(rust,tokio)
        // 共现 >= 2 的对：
        //  (rust,tokio): mem1+mem3 = 2
        //  (async,rust): mem2+mem3 = 2
        // 其余对仅共现 1 次，不生成走廊
        let memories = vec![
            make_mem("rust tokio framework"),
            make_mem("rust async runtime"),
            make_mem("rust tokio async"),
        ];

        let hallways = graph.compute_hallways_for_wing("tech", &memories);

        assert_eq!(
            hallways.len(),
            2,
            "expected 2 hallways, got {}: {:?}",
            hallways.len(),
            hallways
        );

        for h in &hallways {
            assert!(h.co_occurrence_count >= 2);
            assert_eq!(h.wing, "tech");
        }

        // 验证 rust-tokio 对存在（无序）
        let has_rust_tokio = hallways.iter().any(|h| {
            (h.entity_a == "rust" && h.entity_b == "tokio")
                || (h.entity_a == "tokio" && h.entity_b == "rust")
        });
        assert!(has_rust_tokio);

        // 验证 async-rust 对存在（无序）
        let has_async_rust = hallways.iter().any(|h| {
            (h.entity_a == "async" && h.entity_b == "rust")
                || (h.entity_a == "rust" && h.entity_b == "async")
        });
        assert!(has_async_rust);
    }

    #[test]
    fn test_compute_hallways_filters_short_tokens() {
        let graph = PalaceGraph::new();
        // 短 token (len <= 3) 应被过滤：the(3), cat(3), dog(3)
        let memories = vec![make_mem("the cat dog"), make_mem("the cat dog")];
        let hallways = graph.compute_hallways_for_wing("w", &memories);
        assert!(hallways.is_empty());
    }

    #[test]
    fn test_compute_hallways_no_cooccur_below_two() {
        let graph = PalaceGraph::new();
        // 只共现一次的对不应生成走廊
        let memories = vec![
            make_mem("rust tokio"),    // (rust,tokio): 1
            make_mem("python django"), // (django,python): 1
        ];
        let hallways = graph.compute_hallways_for_wing("w", &memories);
        assert!(hallways.is_empty());
    }

    #[test]
    fn test_compute_hallways_case_insensitive() {
        let graph = PalaceGraph::new();
        // 大小写不同应视为同一 token
        let memories = vec![make_mem("Rust Tokio"), make_mem("rust tokio")];
        let hallways = graph.compute_hallways_for_wing("w", &memories);
        assert_eq!(hallways.len(), 1);
        assert_eq!(hallways[0].co_occurrence_count, 2);
    }

    #[test]
    fn test_delete_hallway() {
        let mut graph = PalaceGraph::new();
        graph.add_hallway("w", "a", "b");
        let id = graph.hallways[0].id.clone();

        assert!(graph.delete_hallway(&id));
        assert_eq!(graph.hallways.len(), 0);
        // 再次删除返回 false
        assert!(!graph.delete_hallway(&id));
    }

    #[test]
    fn test_delete_tunnel() {
        let mut graph = PalaceGraph::new();
        let id = graph.add_tunnel("w1", "r1", "w2", "r2", "l");

        assert!(graph.delete_tunnel(&id));
        assert_eq!(graph.tunnels.len(), 0);
        assert!(!graph.delete_tunnel(&id));
    }

    #[test]
    fn test_stats() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("w1", "");
        graph.add_wing("w2", "");
        graph.add_room("r1", "w1");
        graph.add_room("r2", "w2");
        graph.add_hallway("w1", "a", "b");
        graph.add_tunnel("w1", "r1", "w2", "r2", "l");

        let stats = graph.stats();
        assert_eq!(stats.wing_count, 2);
        assert_eq!(stats.room_count, 2);
        assert_eq!(stats.hallway_count, 1);
        assert_eq!(stats.tunnel_count, 1);
    }

    #[test]
    fn test_default_impl() {
        let graph = PalaceGraph::default();
        let stats = graph.stats();
        assert_eq!(stats.wing_count, 0);
        assert_eq!(stats.room_count, 0);
        assert_eq!(stats.hallway_count, 0);
        assert_eq!(stats.tunnel_count, 0);
    }

    #[test]
    fn test_struct_serialization_roundtrip() {
        let mut graph = PalaceGraph::new();
        graph.add_wing("w", "desc");
        graph.add_room("r", "w");
        graph.add_hallway("w", "a", "b");
        graph.add_tunnel("w", "r", "w2", "r2", "label");

        // Wing 序列化往返
        let wing_json = serde_json::to_string(graph.wings.get("w").unwrap()).unwrap();
        let wing: Wing = serde_json::from_str(&wing_json).unwrap();
        assert_eq!(wing.name, "w");
        assert_eq!(wing.description, "desc");

        // Room 序列化往返
        let room_json = serde_json::to_string(graph.rooms.get("r").unwrap()).unwrap();
        let room: Room = serde_json::from_str(&room_json).unwrap();
        assert_eq!(room.name, "r");
        assert_eq!(room.wing, "w");

        // Hallway 序列化往返
        let h_json = serde_json::to_string(&graph.hallways[0]).unwrap();
        let h: Hallway = serde_json::from_str(&h_json).unwrap();
        assert_eq!(h.entity_a, "a");
        assert_eq!(h.entity_b, "b");
        assert_eq!(h.co_occurrence_count, 1);

        // Tunnel 序列化往返
        let t_json = serde_json::to_string(&graph.tunnels[0]).unwrap();
        let t: Tunnel = serde_json::from_str(&t_json).unwrap();
        assert_eq!(t.label, "label");
        assert_eq!(t.source_wing, "w");
        assert_eq!(t.target_room, "r2");

        // Stats 序列化往返
        let stats = graph.stats();
        let s_json = serde_json::to_string(&stats).unwrap();
        let s: PalaceGraphStats = serde_json::from_str(&s_json).unwrap();
        assert_eq!(s.wing_count, stats.wing_count);
        assert_eq!(s.room_count, stats.room_count);
        assert_eq!(s.hallway_count, stats.hallway_count);
        assert_eq!(s.tunnel_count, stats.tunnel_count);
    }
}
