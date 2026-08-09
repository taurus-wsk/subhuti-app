//! # Blender 领域专家
//!
//! 示例：展示如何在领域层开发垂直场景专家。
//!
//! 领域层只依赖领域层自身定义的接口（DomainExpert、DomainLlm），
//! 不依赖任何外部框架类型（subhuti、HTTP、数据库等）。
//!
//! 这段代码可以整体提取到独立 crate，编译为 WASM 插件，无需修改核心逻辑。

use async_trait::async_trait;

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
        // 示例：从数据仓库加载专家配置或历史记录
        let config_key = format!(
            "blender_expert_config_{}",
            exec_ctx.ctx.session_id.as_deref().unwrap_or("default")
        );
        let config = exec_ctx.repository.load(&config_key).await?;

        // 如果有配置，使用配置；否则使用默认提示词
        let system_prompt = if let Some(cfg) = config {
            tracing::debug!("加载到 Blender 专家配置: {}", cfg);
            format!("你是一位 Blender 3D 动画制作专家。{}", cfg)
        } else {
            "你是一位 Blender 3D 动画制作专家。\
            你精通建模、材质、灯光、动画、渲染、粒子系统、几何节点等各个方面。\
            你会根据用户的问题，提供详细的操作步骤和技巧。\
            回答时请使用中文，并在需要时提供 Python 脚本代码。"
                .to_string()
        };

        // 构建消息列表
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

        // 示例：保存专家执行结果到数据仓库
        let result_key = format!(
            "blender_expert_result_{}",
            exec_ctx.ctx.session_id.as_deref().unwrap_or("default")
        );
        if let Err(e) = exec_ctx.repository.save(&result_key, &response).await {
            tracing::warn!("保存 Blender 专家结果失败: {}", e);
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

        // 保存执行结果
        let result_key = format!(
            "blender_skill_result_{}_{}",
            skill_id,
            exec_ctx.ctx.session_id.as_deref().unwrap_or("default")
        );
        if let Err(e) = exec_ctx.repository.save(&result_key, &response).await {
            tracing::warn!("保存 Blender 技能执行结果失败: {}", e);
        }

        Ok(response)
    }
}
