//! # 藏经阁召回统一配置

/// 藏经阁整体召回入口配置
#[derive(Debug, Clone)]
pub struct LibraryRetrieveConfig {
    // -------- 基础检索参数 --------
    pub base_top_k: usize,

    // -------- 静态空间通路 SpaceDepthStrategy --------
    pub space_enable: bool,
    pub space_max_tree_distance: u32,
    pub space_per_level_limit: usize,

    // -------- 图谱公共配置 --------
    pub graph_max_depth: usize,
    pub graph_max_global_result: usize,
    pub graph_decay_factor: f32,
    pub enable_manual_edge: bool,
    pub enable_learned_edge: bool,
    pub learned_edge_scale: f32,

    // -------- 二阶图谱（空间切片投喂图谱）开关 --------
    pub graph_second_pass_from_space: bool,
    pub second_pass_score_decay: f32,
    pub second_pass_max_result: usize,
}

/// 默认标准配置（日常检索）
impl Default for LibraryRetrieveConfig {
    fn default() -> Self {
        Self {
            base_top_k: 8,

            space_enable: true,
            space_max_tree_distance: 0,
            space_per_level_limit: 8,

            graph_max_depth: 2,
            graph_max_global_result: 12,
            graph_decay_factor: 0.7,
            enable_manual_edge: true,
            enable_learned_edge: true,
            learned_edge_scale: 0.4,

            graph_second_pass_from_space: false,
            second_pass_score_decay: 0.65,
            second_pass_max_result: 8,
        }
    }
}

impl LibraryRetrieveConfig {
    /// 深度问答增强配置（打开二阶图谱）
    pub fn deep_qa() -> Self {
        Self {
            graph_second_pass_from_space: true,
            ..Self::default()
        }
    }

    /// 精准检索模式：仅同抽屉，无图谱
    pub fn precise() -> Self {
        Self {
            space_max_tree_distance: 0,
            enable_manual_edge: false,
            enable_learned_edge: false,
            graph_second_pass_from_space: false,
            ..Self::default()
        }
    }

    /// 模块内宽松检索：同柜子，无二阶图谱
    pub fn module_relaxed() -> Self {
        Self {
            space_max_tree_distance: 1,
            graph_second_pass_from_space: false,
            ..Self::default()
        }
    }
}
