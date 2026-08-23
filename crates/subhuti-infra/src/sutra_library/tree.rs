//! # 树结构管理
//!
//! SlotTree: 基于 slotmap 的高效树结构，O(1) 节点查找，O(1) 父/子访问。
//! TreeValidator: 树结构合法性校验（防环、防跨挂载、单根约束）、路径计算、子树操作。
//!
//! 双层设计：
//! - SlotTree 负责树结构存储与基本操作（增删改查）
//! - TreeValidator 负责校验逻辑（环检测、挂载检查等）

use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;

new_key_type! {
    /// 树节点键（slotmap 句柄）
    pub struct TreeNodeKey;
}

/// 树节点（slotmap 存储）
#[derive(Debug, Clone)]
pub struct SlotTreeNode {
    /// 节点 ID（对应 MemoryNode.node_id）
    pub node_id: String,
    /// 父节点键
    pub parent: Option<TreeNodeKey>,
    /// 子节点键列表
    pub children: Vec<TreeNodeKey>,
    /// 完整路径（如 /Blender/阵列修改器）
    pub path: String,
    /// 树深度（根为 0）
    pub depth: u32,
    /// 所属集合 ID
    pub collection_id: String,
}

/// 基于 slotmap 的树结构
///
/// 提供 O(1) 的节点查找、父/子访问，以及高效的子树操作。
pub struct SlotTree {
    /// slotmap 存储所有节点
    nodes: SlotMap<TreeNodeKey, SlotTreeNode>,
    /// node_id → TreeNodeKey 的快速反向索引
    by_node_id: HashMap<String, TreeNodeKey>,
}

impl SlotTree {
    pub fn new() -> Self {
        Self {
            nodes: SlotMap::with_key(),
            by_node_id: HashMap::new(),
        }
    }

    /// 添加节点到树中
    ///
    /// 如果节点已存在，则会更新其信息（parent, path, depth, collection_id）。
    /// 返回节点的 TreeNodeKey。
    pub fn add_node(
        &mut self,
        node_id: &str,
        parent_id: Option<&str>,
        title: &str,
        collection_id: &str,
    ) -> TreeNodeKey {
        let (parent_key, depth, path) = if let Some(pid) = parent_id {
            if let Some(&pk) = self.by_node_id.get(pid) {
                let parent_depth = self.nodes[pk].depth;
                let parent_path = self.nodes[pk].path.clone();
                (
                    Some(pk),
                    parent_depth + 1,
                    format!("{}/{}", parent_path, title),
                )
            } else {
                (None, 0, format!("/{}", title))
            }
        } else {
            (None, 0, format!("/{}", title))
        };

        if let Some(&existing_key) = self.by_node_id.get(node_id) {
            // 更新现有节点
            let old_parent_key = self.nodes.get(existing_key).and_then(|n| n.parent);
            let parent_changed = old_parent_key != parent_key;

            // 如果父节点变了，需要从旧父节点移除
            if parent_changed {
                if let Some(old_parent) = old_parent_key {
                    if let Some(old_p) = self.nodes.get_mut(old_parent) {
                        old_p.children.retain(|c| *c != existing_key);
                    }
                }
            }

            // 更新节点信息
            if let Some(node) = self.nodes.get_mut(existing_key) {
                if parent_changed {
                    node.parent = parent_key;
                }
                node.path = path;
                node.depth = depth;
                node.collection_id = collection_id.to_string();
            }

            // 添加到新父节点
            if parent_changed {
                if let Some(new_parent) = parent_key {
                    if let Some(p) = self.nodes.get_mut(new_parent) {
                        p.children.push(existing_key);
                    }
                }
            }

            existing_key
        } else {
            // 添加新节点
            let key = self.nodes.insert(SlotTreeNode {
                node_id: node_id.to_string(),
                parent: parent_key,
                children: Vec::new(),
                path,
                depth,
                collection_id: collection_id.to_string(),
            });
            self.by_node_id.insert(node_id.to_string(), key);

            // 添加到父节点的子节点列表
            if let Some(pk) = parent_key {
                if let Some(parent) = self.nodes.get_mut(pk) {
                    parent.children.push(key);
                }
            }

            key
        }
    }

    /// 移除节点及其子树
    ///
    /// 返回移除的所有 node_id 列表（包括自身）。
    pub fn remove_node(&mut self, node_id: &str) -> Vec<String> {
        let mut removed = Vec::new();
        if let Some(&key) = self.by_node_id.get(node_id) {
            // 先递归收集子树
            let subtree_keys = self.collect_subtree_keys(key);

            let mut removed_ids = Vec::new();
            let mut parent_removals: Vec<(TreeNodeKey, TreeNodeKey)> = Vec::new();

            for &sk in &subtree_keys {
                if let Some(node) = self.nodes.get(sk) {
                    if let Some(pk) = node.parent {
                        parent_removals.push((pk, sk));
                    }
                    removed_ids.push(node.node_id.clone());
                    self.by_node_id.remove(&node.node_id);
                }
            }

            // 从父节点移除子节点引用
            for (pk, sk) in &parent_removals {
                if let Some(parent) = self.nodes.get_mut(*pk) {
                    parent.children.retain(|c| c != sk);
                }
            }

            // 删除节点
            for &sk in &subtree_keys {
                self.nodes.remove(sk);
            }

            removed = removed_ids;
        }
        removed
    }

    /// 递归收集子树所有键
    fn collect_subtree_keys(&self, root: TreeNodeKey) -> Vec<TreeNodeKey> {
        let mut keys = vec![root];
        if let Some(node) = self.nodes.get(root) {
            for child in &node.children {
                keys.extend(self.collect_subtree_keys(*child));
            }
        }
        keys
    }

    /// 获取子树所有 node_id（递归）
    pub fn collect_subtree_ids(&self, node_id: &str) -> Vec<String> {
        let mut ids = Vec::new();
        if let Some(&key) = self.by_node_id.get(node_id) {
            for k in self.collect_subtree_keys(key) {
                if let Some(node) = self.nodes.get(k) {
                    ids.push(node.node_id.clone());
                }
            }
        }
        ids
    }

    /// 获取子节点列表
    pub fn get_children(&self, node_id: &str) -> Vec<String> {
        let mut children = Vec::new();
        if let Some(&key) = self.by_node_id.get(node_id) {
            if let Some(node) = self.nodes.get(key) {
                for &child_key in &node.children {
                    if let Some(child) = self.nodes.get(child_key) {
                        children.push(child.node_id.clone());
                    }
                }
            }
        }
        children
    }

    /// 获取父节点 ID
    pub fn get_parent(&self, node_id: &str) -> Option<String> {
        self.by_node_id.get(node_id).and_then(|&key| {
            self.nodes.get(key).and_then(|n| {
                n.parent
                    .and_then(|pk| self.nodes.get(pk).map(|p| p.node_id.clone()))
            })
        })
    }

    /// 获取节点路径
    pub fn get_path(&self, node_id: &str) -> Option<String> {
        self.by_node_id
            .get(node_id)
            .and_then(|&key| self.nodes.get(key).map(|n| n.path.clone()))
    }

    /// 获取节点深度
    pub fn get_depth(&self, node_id: &str) -> Option<u32> {
        self.by_node_id
            .get(node_id)
            .and_then(|&key| self.nodes.get(key).map(|n| n.depth))
    }

    /// 检查是否形成环（从 parent 向上追溯到 root，是否到达 node）
    pub fn would_create_cycle(&self, node_id: &str, new_parent_id: &str) -> bool {
        let mut current = new_parent_id.to_string();
        loop {
            if current == node_id {
                return true;
            }
            match self.get_parent(&current) {
                Some(pid) => current = pid,
                None => return false,
            }
        }
    }

    /// 检查节点是否已经挂载到指定集合
    pub fn is_already_mounted(&self, node_id: &str, collection_id: &str) -> bool {
        self.by_node_id.get(node_id).map_or(false, |&key| {
            self.nodes.get(key).map_or(false, |n| {
                n.collection_id == collection_id && n.parent.is_some()
            })
        })
    }

    /// 检查集合是否有根节点
    pub fn has_root_in_collection(&self, collection_id: &str) -> bool {
        self.nodes
            .iter()
            .any(|(_, n)| n.collection_id == collection_id && n.parent.is_none())
    }

    /// 获取节点数
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 获取所有节点 ID
    pub fn all_node_ids(&self) -> Vec<String> {
        self.nodes.iter().map(|(_, n)| n.node_id.clone()).collect()
    }

    /// 获取节点所属集合 ID
    pub fn get_collection_id(&self, node_id: &str) -> Option<String> {
        self.by_node_id
            .get(node_id)
            .and_then(|&key| self.nodes.get(key).map(|n| n.collection_id.clone()))
    }

    /// 计算路径
    ///
    /// 不修改树结构，仅根据父节点和标题计算路径。
    pub fn compute_path(&self, parent_id: Option<&str>, title: &str) -> String {
        match parent_id {
            Some(pid) => {
                if let Some(path) = self.get_path(pid) {
                    format!("{}/{}", path, title)
                } else {
                    format!("/{}", title)
                }
            }
            None => format!("/{}", title),
        }
    }

    /// 计算深度
    pub fn compute_depth(&self, parent_id: Option<&str>) -> u32 {
        match parent_id {
            Some(pid) => self.get_depth(pid).unwrap_or(0) + 1,
            None => 0,
        }
    }
}

/// 树结构校验器（封装 SlotTree 校验逻辑）
pub struct TreeValidator;

impl TreeValidator {
    /// 检查是否形成环（从 parent 向上追溯到 root，是否到达 node）
    pub fn would_create_cycle(tree: &SlotTree, node_id: &str, new_parent_id: &str) -> bool {
        tree.would_create_cycle(node_id, new_parent_id)
    }

    /// 检查节点是否已经是某个集合的子节点
    pub fn is_already_mounted(tree: &SlotTree, node_id: &str, collection_id: &str) -> bool {
        tree.is_already_mounted(node_id, collection_id)
    }

    /// 确保集合只有一个根节点
    pub fn has_root_in_collection(tree: &SlotTree, collection_id: &str) -> bool {
        tree.has_root_in_collection(collection_id)
    }

    /// 完整校验：挂载节点前检查
    pub fn validate_mount(
        tree: &SlotTree,
        node_id: &str,
        collection_id: &str,
        new_parent_id: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(parent_id) = new_parent_id {
            if tree.would_create_cycle(node_id, parent_id) {
                anyhow::bail!("挂载操作会形成环: node={} parent={}", node_id, parent_id);
            }
            // 检查父节点集合一致性
            if let Some(parent_collection) = tree.get_collection_id(parent_id) {
                if parent_collection != collection_id {
                    anyhow::bail!(
                        "跨集合挂载不允许: node_collection={} parent_collection={}",
                        collection_id,
                        parent_collection
                    );
                }
            }
        }
        Ok(())
    }

    /// 计算节点路径
    pub fn compute_path(tree: &SlotTree, parent_id: Option<&str>, title: &str) -> String {
        tree.compute_path(parent_id, title)
    }

    /// 计算深度
    pub fn compute_depth(tree: &SlotTree, parent_id: Option<&str>) -> u32 {
        tree.compute_depth(parent_id)
    }

    /// 获取子树所有节点 ID（递归）
    pub fn collect_subtree_ids(tree: &SlotTree, node_id: &str) -> Vec<String> {
        tree.collect_subtree_ids(node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_tree_add_and_get() {
        let mut tree = SlotTree::new();
        let k1 = tree.add_node("root", None, "根", "col1");
        let k2 = tree.add_node("child1", Some("root"), "子1", "col1");
        let k3 = tree.add_node("child2", Some("root"), "子2", "col1");

        assert_eq!(tree.len(), 3);
        assert_eq!(tree.get_path("root"), Some("/根".to_string()));
        assert_eq!(tree.get_path("child1"), Some("/根/子1".to_string()));
        assert_eq!(tree.get_depth("root"), Some(0));
        assert_eq!(tree.get_depth("child1"), Some(1));

        let children = tree.get_children("root");
        assert_eq!(children.len(), 2);
        assert!(children.contains(&"child1".to_string()));
        assert!(children.contains(&"child2".to_string()));
    }

    #[test]
    fn test_slot_tree_cycle_detection() {
        let mut tree = SlotTree::new();
        tree.add_node("a", None, "A", "col1");
        tree.add_node("b", Some("a"), "B", "col1");
        tree.add_node("c", Some("b"), "C", "col1");

        // a → b → c，把 a 挂到 c 下会形成环
        assert!(tree.would_create_cycle("a", "c"));
        // 反过来不会
        assert!(!tree.would_create_cycle("c", "a"));
    }

    #[test]
    fn test_slot_tree_remove_subtree() {
        let mut tree = SlotTree::new();
        tree.add_node("root", None, "根", "col1");
        tree.add_node("child1", Some("root"), "子1", "col1");
        tree.add_node("grandchild", Some("child1"), "孙", "col1");

        assert_eq!(tree.len(), 3);
        let removed = tree.remove_node("child1");
        assert_eq!(removed.len(), 2); // child1 + grandchild
        assert!(removed.contains(&"child1".to_string()));
        assert!(removed.contains(&"grandchild".to_string()));
        assert_eq!(tree.len(), 1); // 只剩 root
    }

    #[test]
    fn test_slot_tree_collect_subtree() {
        let mut tree = SlotTree::new();
        tree.add_node("root", None, "根", "col1");
        tree.add_node("a", Some("root"), "A", "col1");
        tree.add_node("b", Some("a"), "B", "col1");
        tree.add_node("c", Some("a"), "C", "col1");

        let subtree = tree.collect_subtree_ids("a");
        assert_eq!(subtree.len(), 3); // a + b + c
        assert!(subtree.contains(&"a".to_string()));
        assert!(subtree.contains(&"b".to_string()));
        assert!(subtree.contains(&"c".to_string()));
    }

    #[test]
    fn test_slot_tree_update_node() {
        let mut tree = SlotTree::new();
        tree.add_node("node1", None, "旧标题", "col1");
        assert_eq!(tree.get_path("node1"), Some("/旧标题".to_string()));

        // 更新节点（重新 add）
        tree.add_node("node1", None, "新标题", "col1");
        assert_eq!(tree.get_path("node1"), Some("/新标题".to_string()));
    }

    #[test]
    fn test_tree_validator() {
        let mut tree = SlotTree::new();
        tree.add_node("a", None, "A", "col1");
        tree.add_node("b", Some("a"), "B", "col1");

        assert!(TreeValidator::would_create_cycle(&tree, "a", "b"));
        assert!(!TreeValidator::would_create_cycle(&tree, "b", "a"));

        // 挂载校验
        assert!(TreeValidator::validate_mount(&tree, "a", "col1", Some("b")).is_err());
        assert!(TreeValidator::validate_mount(&tree, "b", "col1", None).is_ok());
    }
}
