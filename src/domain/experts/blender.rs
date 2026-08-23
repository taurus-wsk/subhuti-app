//! # Blender 领域专家
//!
//! 示例：展示如何在领域层开发垂直场景专家。
//!
//! 领域层只依赖领域层自身定义的接口（DomainExpert、DomainLlm），
//! 不依赖任何外部框架类型（subhuti、HTTP、数据库等）。
//!
//! 这段代码可以整体提取到独立 crate，编译为 WASM 插件，无需修改核心逻辑。

use async_trait::async_trait;

use crate::application::observer::{record_fn_log, LogLevel};
use crate::domain::traits::{
    DomainExecutionContext, DomainExpert, DomainMessage, DomainResult, DomainRole, DomainSkill,
};

/// Blender 动画制作专家
///
/// 领域专家实现：纯业务逻辑，不依赖框架类型。
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
            skills: vec![
                DomainSkill {
                    id: "blender-chat".into(),
                    name: "聊天对话".into(),
                    description: "与 Blender 专家进行自然语言对话，解答关于 Blender 的各种问题"
                        .into(),
                    parameters: vec!["问题".into()],
                },
                DomainSkill {
                    id: "blender-modeling".into(),
                    name: "3D建模".into(),
                    description: "创建3D模型，包括基础几何体、雕刻、拓扑优化等".into(),
                    parameters: vec!["模型类型".into(), "风格".into()],
                },
                DomainSkill {
                    id: "blender-material".into(),
                    name: "材质制作".into(),
                    description: "创建和调整材质，包括PBR材质、节点材质等".into(),
                    parameters: vec!["材质类型".into(), "质感".into()],
                },
                DomainSkill {
                    id: "blender-animation".into(),
                    name: "动画制作".into(),
                    description: "制作关键帧动画、粒子动画、物理模拟等".into(),
                    parameters: vec!["动画类型".into(), "时长".into()],
                },
                DomainSkill {
                    id: "blender-render".into(),
                    name: "渲染输出".into(),
                    description: "设置渲染参数，输出高质量图像和视频".into(),
                    parameters: vec!["渲染器".into(), "分辨率".into(), "采样".into()],
                },
            ],
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
        // ── 藏经阁记忆引擎使用示例 ──
        // 如果配置了藏经阁引擎，专家可以自动存取结构化记忆
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

            // 使用新版召回流水线搜索相关记忆（五阶段：BaseSearch → Space → Graph → 合并 → 排序）
            let history = sutra.library_retrieve(&exec_ctx.ctx.input, 3).await;
            if !history.contains("未找到") && !history.contains("⚠️") {
                record_fn_log(
                    None,
                    "",
                    LogLevel::Debug,
                    format!(
                        "Blender 新版召回检索到相关记忆:\n{}",
                        &history[..history.len().min(200)]
                    ),
                    None,
                );
                retrieved_history = Some(history);
            }
        }

        // 构建系统提示词，将检索到的历史知识注入上下文
        let mut system_prompt = String::from(
            "你是一位 Blender 3D 动画制作专家。\
            你精通建模、材质、灯光、动画、渲染、粒子系统、几何节点等各个方面。\
            你会根据用户的问题，提供详细的操作步骤和技巧。\
            回答时请使用中文，并在需要时提供 Python 脚本代码。",
        );

        // 如果检索到相关历史记忆，注入到 system prompt 中
        if let Some(history) = &retrieved_history {
            // 清理 Markdown 格式，保留纯文本信息
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

        // 从数据仓库加载专家配置并覆盖提示词
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
            system_prompt = format!("你是一位 Blender 3D 动画制作专家。{}", cfg);
        }

        // 构建消息列表
        let user_input = exec_ctx.ctx.input.clone();
        let messages = vec![
            DomainMessage {
                role: DomainRole::System,
                content: system_prompt,
            },
            DomainMessage {
                role: DomainRole::User,
                content: exec_ctx.ctx.input,
            },
        ];

        // 调用 LLM
        let response = exec_ctx.llm.chat(messages).await?;

        // 记录执行日志到反馈分析器（反馈闭环入口）
        if let Some(ref sutra) = exec_ctx.sutra_library {
            let session_id = exec_ctx.ctx.session_id.as_deref().unwrap_or("default");
            sutra.record_execution(
                &user_input,
                true, // task_success: LLM 返回即视为成功
                "default",
                "blender",
                Some(session_id.to_string()),
            );
        }

        // 示例：保存专家执行结果到数据仓库
        let result_key = format!(
            "blender_expert_result_{}",
            exec_ctx.ctx.session_id.as_deref().unwrap_or("default")
        );
        if let Err(e) = exec_ctx.repository.save(&result_key, &response).await {
            record_fn_log(
                None,
                "",
                LogLevel::Warn,
                format!("保存 Blender 专家结果失败: {}", e),
                None,
            );
        }

        Ok(response)
    }

    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        // 根据技能ID选择不同的执行逻辑
        let system_prompt = match skill_id {
            "blender-chat" => {
                "你是一位专业的 Blender 3D 动画制作专家。\
                请用自然、友好的方式回答用户关于 Blender 的各种问题。\
                可以涵盖建模、材质、动画、渲染、脚本等各个方面。\
                回答时请使用中文，并在需要时提供具体的操作步骤和代码示例。"
            }
            "blender-modeling" => {
                "你是一位专业的 Blender 3D 建模专家。\
                用户正在询问关于建模的问题，可能涉及：\
                - 基础几何体创建和编辑\n\
                - 雕刻和塑形\n\
                - 拓扑优化\n\
                - UV 展开\n\
                - 硬表面建模\n\
                - 角色建模\n\
                请提供详细的操作步骤和实用技巧。"
            }
            "blender-material" => {
                "你是一位专业的 Blender 材质专家。\
                用户正在询问关于材质制作的问题，可能涉及：\
                - PBR 材质设置\n\
                - 节点材质创建\n\
                - 纹理贴图应用\n\
                - 材质属性调整\n\
                请提供详细的节点连接方案和参数设置建议。"
            }
            "blender-animation" => {
                "你是一位专业的 Blender 动画专家。\
                用户正在询问关于动画制作的问题，可能涉及：\
                - 关键帧动画\n\
                - 角色绑定和动画\n\
                - 粒子系统\n\
                - 物理模拟\n\
                - 驱动关键帧\n\
                请提供详细的动画制作流程和技巧。"
            }
            "blender-render" => {
                "你是一位专业的 Blender 渲染专家。\
                用户正在询问关于渲染输出的问题，可能涉及：\
                - Cycles / Eevee 渲染器选择\n\
                - 渲染参数设置\n\
                - 光照设置\n\
                - 渲染优化\n\
                - 输出格式和分辨率\n\
                请提供详细的渲染设置建议和优化技巧。"
            }
            _ => {
                // 未知技能，使用默认提示词
                "你是一位 Blender 3D 动画制作专家。\
                你精通建模、材质、灯光、动画、渲染、粒子系统、几何节点等各个方面。\
                请根据用户的问题提供详细的解答。"
            }
        };

        // 构建消息列表（包含技能参数）
        let user_input = exec_ctx.ctx.input.clone();
        let messages = vec![
            DomainMessage {
                role: DomainRole::System,
                content: system_prompt.to_string(),
            },
            DomainMessage {
                role: DomainRole::User,
                content: format!("技能参数: {}\n\n{}", params, exec_ctx.ctx.input),
            },
        ];

        // 调用 LLM
        let response = exec_ctx.llm.chat(messages).await?;

        // 记录执行日志到反馈分析器
        if let Some(ref sutra) = exec_ctx.sutra_library {
            let session_id = exec_ctx.ctx.session_id.as_deref().unwrap_or("default");
            sutra.record_execution(
                &user_input,
                true,
                "default",
                "blender",
                Some(session_id.to_string()),
            );
        }

        // 保存执行结果
        let result_key = format!(
            "blender_skill_result_{}_{}",
            skill_id,
            exec_ctx.ctx.session_id.as_deref().unwrap_or("default")
        );
        if let Err(e) = exec_ctx.repository.save(&result_key, &response).await {
            record_fn_log(
                None,
                "",
                LogLevel::Warn,
                format!("保存 Blender 技能执行结果失败: {}", e),
                None,
            );
        }

        Ok(response)
    }
}
