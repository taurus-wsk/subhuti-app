//! # Blender 领域专家
//!
//! 示例：展示如何在领域层开发垂直场景专家。
//!
//! 领域层只依赖领域层自身定义的接口（DomainExpert、DomainLlm），
//! 不依赖任何外部框架类型（subhuti、HTTP、数据库等）。
//!
//! 这段代码可以整体提取到独立 crate，编译为 WASM 插件，无需修改核心逻辑。
//!
//! 执行模型：**Skill = Flow + Tool**。`run` 走固定 React 模板
//! `analyze→plan→edit→verify→done`（完整五阶段），`execute_skill` 走
//! `analyze→edit→done`（三阶段）择取对应技能提示词。节点逻辑由
//! `BlenderFlowExecutor`（FlowNodeExecutor）提供，`FlowRunner` 负责顺序与事件。

use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::orchestrator::{FlowContext, FlowNodeResult, ReactStage};

use crate::application::observer::{record_fn_log, LogLevel};
use crate::domain::events::DomainEvent;
use crate::domain::flow_exec::{flow_str, run_react_flow, FlowKeyId, FlowNodeExecutor};
use crate::domain::traits::{
    chat_stream_to_progress, DomainContext, DomainExecutionContext, DomainExpert, DomainMessage,
    DomainResult, DomainRole, DomainSkill,
};

/// Blender 中间结果 key（`FlowKeyId` / `flow_str` 共用定义见
/// [`crate::domain::flow_exec`]）
#[derive(Clone, Copy)]
enum BlenderKey {
    Sys,
}

impl FlowKeyId for BlenderKey {
    fn as_str(&self) -> &'static str {
        match self {
            BlenderKey::Sys => "blender.sys",
        }
    }
}

// Blender 技能种类（决定 Flow 各节点如何执行）
//
// 这是所有技能的**唯一注册表**：id / 名称 / 描述 / 参数 / 专属提示词都从这里派生，
// `skills()`（元数据）与 `execute_skill`（调度）共用同一份定义。
//
// 由 [`skill_catalog!`] 宏从一张数据表生成；新增技能只需补一行。
skill_catalog!(
    BlenderSkillKind,
    system_prompt, &'static str,
    {
        Chat {
            id: "blender-chat",
            name: "聊天对话",
            desc: "与 Blender 专家进行自然语言对话，解答关于 Blender 的各种问题",
            params: ["问题"],
            custom: r#"你是一位专业的 Blender 3D 动画制作专家。
请用自然、友好的方式回答用户关于 Blender 的各种问题。
可以涵盖建模、材质、动画、渲染、脚本等各个方面。
回答时请使用中文，并在需要时提供具体的操作步骤和代码示例。"#,
        },
        Modeling {
            id: "blender-modeling",
            name: "3D建模",
            desc: "创建3D模型，包括基础几何体、雕刻、拓扑优化等",
            params: ["模型类型", "风格"],
            custom: r#"你是一位专业的 Blender 3D 建模专家。
用户正在询问关于建模的问题，可能涉及：
- 基础几何体创建和编辑
- 雕刻和塑形
- 拓扑优化
- UV 展开
- 硬表面建模
- 角色建模
请提供详细的操作步骤和实用技巧。"#,
        },
        Material {
            id: "blender-material",
            name: "材质制作",
            desc: "创建和调整材质，包括PBR材质、节点材质等",
            params: ["材质类型", "质感"],
            custom: r#"你是一位专业的 Blender 材质专家。
用户正在询问关于材质制作的问题，可能涉及：
- PBR 材质设置
- 节点材质创建
- 纹理贴图应用
- 材质属性调整
请提供详细的节点连接方案和参数设置建议。"#,
        },
        Animation {
            id: "blender-animation",
            name: "动画制作",
            desc: "制作关键帧动画、粒子动画、物理模拟等",
            params: ["动画类型", "时长"],
            custom: r#"你是一位专业的 Blender 动画专家。
用户正在询问关于动画制作的问题，可能涉及：
- 关键帧动画
- 角色绑定和动画
- 粒子系统
- 物理模拟
- 驱动关键帧
请提供详细的动画制作流程和技巧。"#,
        },
        Render {
            id: "blender-render",
            name: "渲染输出",
            desc: "设置渲染参数，输出高质量图像和视频",
            params: ["渲染器", "分辨率", "采样"],
            custom: r#"你是一位专业的 Blender 渲染专家。
用户正在询问关于渲染输出的问题，可能涉及：
- Cycles / Eevee 渲染器选择
- 渲染参数设置
- 光照设置
- 渲染优化
- 输出格式和分辨率
请提供详细的渲染设置建议和优化技巧。"#,
        },
    }
);

/// 把 BlenderExpert 挂到固定 React 模板上。
///
/// `skill`：`None` 表示通用 `run`（完整五阶段），`Some(id)` 表示技能执行
/// （三阶段择取对应提示词）。节点逻辑统一交由 `BlenderExpert::flow_pure /
/// flow_llm` 分派。
struct BlenderFlowExecutor {
    skill: Option<BlenderSkillKind>,
    expert: Arc<BlenderExpert>,
}

#[async_trait]
impl FlowNodeExecutor for BlenderFlowExecutor {
    async fn execute_pure(
        &self,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        self.expert
            .flow_pure(self.skill, stage, exec_ctx, ctx)
            .await
    }

    async fn execute_llm(
        &self,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        self.expert.flow_llm(self.skill, stage, exec_ctx, ctx).await
    }
}

/// Blender 动画制作专家
///
/// 领域专家实现：纯业务逻辑，不依赖框架类型。
#[derive(Clone)]
pub struct BlenderExpert {
    tags: Vec<String>,
    skills: Vec<DomainSkill>,
}

impl BlenderExpert {
    pub fn new() -> Self {
        Self {
            tags: vec![
                "blender".into(),
                "3D".into(),
                "动画".into(),
                "建模".into(),
                "渲染".into(),
                "材质".into(),
                "节点".into(),
                "粒子".into(),
            ],
            // 技能元数据从唯一注册表 `BlenderSkillKind::ALL` 派生，不手写重复 id/name/desc
            skills: BlenderSkillKind::ALL.iter().map(|k| k.meta()).collect(),
        }
    }

    // ─── Flow 节点逻辑（Skill = Flow + Tool）───────────────────────

    /// 按 技能/通用 构造执行器跑固定 React 模板，返回累积产物
    async fn run_skill_flow(
        &self,
        skill: Option<BlenderSkillKind>,
        exec_ctx: DomainExecutionContext,
        stages: &[ReactStage],
    ) -> DomainResult<String> {
        let ex = Arc::new(BlenderFlowExecutor {
            skill,
            expert: Arc::new(self.clone()),
        });
        run_react_flow(&exec_ctx, "blender.react", stages, ex).await
    }

    /// 通用 run 的 system prompt 构建：藏经阁记忆检索 + 默认提示词 + 仓库配置覆盖
    async fn build_system_prompt(&self, exec_ctx: &DomainExecutionContext) -> DomainResult<String> {
        let mut retrieved_history: Option<String> = None;
        if let Some(ref sutra) = exec_ctx.sutra_library {
            // 确保 Blender 知识集合存在
            let collections = sutra.list_collections();
            if !collections.contains("blender_knowledge") && !collections.contains("⚠️") {
                sutra.create_collection(
                    "blender_knowledge",
                    "blender",
                    "Blender 3D 建模/动画/材质/渲染知识",
                );
            }

            // 存储当前会话输入到临时记忆
            let session_id = exec_ctx.ctx.session_id.as_deref().unwrap_or("default");
            sutra.add_session(session_id, &exec_ctx.ctx.input, "blender");

            // 发射 MemoryRetrieved（retrieve 阶段）
            exec_ctx
                .trace_context()
                .emit(DomainEvent::MemoryRetrieved {
                    query: exec_ctx.ctx.input.clone(),
                    results_count: 0,
                })
                .await;
            let history = sutra.library_retrieve(&exec_ctx.ctx.input, 3).await;
            if !history.contains("未找到") && !history.contains("⚠️") {
                let preview: String = history.chars().take(200).collect();
                record_fn_log(
                    None,
                    "",
                    LogLevel::Debug,
                    format!("Blender 新版召回检索到相关记忆:\n{}", preview),
                    None,
                );
                retrieved_history = Some(history);
            }
        }

        let mut system_prompt = String::from(
            "你是一位 Blender 3D 动画制作专家。\
            你精通建模、材质、灯光、动画、渲染、粒子系统、几何节点等各个方面。\
            你会根据用户的问题，提供详细的操作步骤和技巧。\
            回答时请使用中文，并在需要时提供 Python 脚本代码。",
        );

        if let Some(history) = &retrieved_history {
            let clean = history
                .replace("🔍", "")
                .replace('*', "")
                .trim()
                .to_string();
            system_prompt.push_str(&format!(
                "\n\n以下是藏经阁中与用户问题相关的历史知识，请参考这些信息来回答：\n{}",
                clean,
            ));
        }

        let config_key = format!(
            "blender_expert_config_{}",
            exec_ctx.ctx.session_id.as_deref().unwrap_or("default")
        );
        if let Some(cfg) = exec_ctx.repository.load(&config_key).await? {
            record_fn_log(
                None,
                "",
                LogLevel::Debug,
                format!("加载到 Blender 专家配置: {}", cfg),
                None,
            );
            system_prompt.push_str(&format!("\n\n{}", cfg));
        }

        Ok(system_prompt)
    }

    /// 纯工具节点：analyze（拼 system prompt）/ verify（无副作用）/ done（取 edit 输出）
    async fn flow_pure(
        &self,
        skill: Option<BlenderSkillKind>,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        match stage {
            ReactStage::Analyze => {
                let sys = match skill {
                    Some(kind) => kind.system_prompt().to_string(),
                    None => self.build_system_prompt(exec_ctx).await?,
                };
                ctx.results.insert(
                    BlenderKey::Sys.as_str().to_string(),
                    FlowNodeResult {
                        output: sys,
                        success: true,
                    },
                );
                Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                })
            }
            ReactStage::Done => {
                // 最终输出 = edit 节点的真实回答，剔除 plan 策略等过程产物
                let edit_out = flow_str(ctx, ReactStage::Edit);
                ctx.output = edit_out;
                Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                })
            }
            // verify 无副作用
            ReactStage::Verify => Ok(FlowNodeResult {
                output: String::new(),
                success: true,
            }),
            ReactStage::Plan | ReactStage::Edit => {
                unreachable!("纯工具节点不能收到 plan/edit 阶段: {stage:?}")
            }
        }
    }

    /// LLM 决策节点：plan（一句话策略）/ edit（真实回答）
    async fn flow_llm(
        &self,
        skill: Option<BlenderSkillKind>,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult> {
        let sys = flow_str(ctx, BlenderKey::Sys);
        match stage {
            ReactStage::Plan => {
                let plan_msg = format!(
                    "请用一句话概括你将如何回答用户的问题。\n\n用户问题：{}",
                    ctx.input
                );
                let messages = vec![
                    DomainMessage {
                        role: DomainRole::System,
                        content: sys,
                    },
                    DomainMessage {
                        role: DomainRole::User,
                        content: plan_msg,
                    },
                ];
                // 策略仅供内部决策，不进入最终输出
                let _strategy = exec_ctx.llm.chat(messages).await?;
                Ok(FlowNodeResult {
                    output: String::new(),
                    success: true,
                })
            }
            ReactStage::Edit => {
                let messages = vec![
                    DomainMessage {
                        role: DomainRole::System,
                        content: sys,
                    },
                    DomainMessage {
                        role: DomainRole::User,
                        content: ctx.input.clone(),
                    },
                ];
                let response =
                    chat_stream_to_progress(&exec_ctx.llm, messages, &exec_ctx.progress_tx).await?;

                // 保存执行结果
                let session_id = exec_ctx.ctx.session_id.as_deref().unwrap_or("default");
                let result_key = match skill {
                    Some(kind) => format!("blender_skill_result_{}_{}", kind.id(), session_id),
                    None => format!("blender_expert_result_{}", session_id),
                };
                if let Err(e) = exec_ctx.repository.save(&result_key, &response).await {
                    record_fn_log(
                        None,
                        "",
                        LogLevel::Warn,
                        format!("保存 Blender 专家结果失败: {}", e),
                        None,
                    );
                }

                Ok(FlowNodeResult {
                    output: response,
                    success: true,
                })
            }
            ReactStage::Analyze | ReactStage::Verify | ReactStage::Done => {
                unreachable!("LLM 决策节点只能收到 plan/edit 阶段: {stage:?}")
            }
        }
    }
}

impl Default for BlenderExpert {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DomainExpert for BlenderExpert {
    fn id(&self) -> &str {
        "blender"
    }

    fn name(&self) -> &str {
        "Blender 动画专家"
    }

    fn tags(&self) -> &[String] {
        &self.tags
    }

    fn skills(&self) -> &[DomainSkill] {
        &self.skills
    }

    async fn run(&self, exec_ctx: DomainExecutionContext) -> DomainResult<String> {
        use ReactStage::*;
        self.run_skill_flow(None, exec_ctx, &[Analyze, Plan, Edit, Verify, Done])
            .await
    }

    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        // 技能参数 + 原始输入合并为 Flow 输入（与原 execute_skill 口径一致）
        let exec_ctx = DomainExecutionContext {
            ctx: DomainContext {
                input: format!("技能参数: {}\n\n{}", params, exec_ctx.ctx.input),
                ..exec_ctx.ctx
            },
            ..exec_ctx
        };
        // 从唯一注册表查技能：id → 种类（消除 match skill_id 魔法字符串）
        let skill = BlenderSkillKind::from_id(skill_id);
        self.run_skill_flow(
            skill,
            exec_ctx,
            &[ReactStage::Analyze, ReactStage::Edit, ReactStage::Done],
        )
        .await
    }
}
