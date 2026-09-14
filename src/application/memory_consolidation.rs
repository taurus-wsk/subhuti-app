//! # 记忆沉淀（Memory Consolidation）
//!
//! 每轮对话结束后，把"值得长期记住的事实"从对话里提炼出来，写进藏经阁并落库。
//!
//! ## 为什么需要这一层
//!
//! 藏经阁原本虽然有 `add_session_memory` → `precipitate_session` 的完整管道，
//! 但生产链路**从未调用沉淀**（唯一调用方 `MemorySkill` 是死代码），
//! 于是 `memory_nodes` 恒为 0、"新会话完全失忆"。本模块把这条链路真正接上。
//!
//! ## 提炼策略（两级，任何一级失败都能降级）
//!
//! 1. **LLM 提炼**：让模型把对话压缩成若干条事实（偏好 / 环境 / 约定 / 参数）。
//! 2. **规则兜底**：LLM 不可用或返回空时，用启发式规则从用户消息里抽候选句。
//!
//! 两级产出的候选都会经过 `is_worth_remembering` 去噪，避免把"你好""谢谢"
//! 这类噪声灌进长期记忆（否则命中率统计会被稀释成永远接近 0）。

use std::sync::Arc;
use subhuti_core::sutra_library::SutraLibraryPort;

/// 沉淀器：编排层持有，在每轮对话成功后调用
pub struct MemoryConsolidator {
    sutra: Option<Arc<dyn SutraLibraryPort>>,
    llm: Option<Arc<dyn subhuti_core::LLM>>,
}

impl MemoryConsolidator {
    pub fn new(
        sutra: Option<Arc<dyn SutraLibraryPort>>,
        llm: Option<Arc<dyn subhuti_core::LLM>>,
    ) -> Self {
        Self { sutra, llm }
    }

    /// 是否可用（藏经阁未装配时全部跳过）
    pub fn is_enabled(&self) -> bool {
        self.sutra.is_some()
    }

    /// 记录一次执行反馈（命中率 / 实体共现）
    ///
    /// 放在编排层统一做，而不是塞进某个专家里：
    /// 此前只有 Blender 专家调用 `record_execution`，Rust 等领域永远是 0 次记录，
    /// 命中率指标形同虚设。上移后对所有领域一视同仁。
    pub fn record_execution(
        &self,
        query: &str,
        task_success: bool,
        domain: &str,
        session_id: &str,
        final_answer: &str,
    ) {
        if let Some(sutra) = &self.sutra {
            sutra.record_execution(
                query,
                task_success,
                "default",
                domain,
                Some(session_id.to_string()),
                final_answer,
            );
        }
    }

    /// 提炼并沉淀本轮对话
    ///
    /// 返回实际落库的记忆条数。全程失败静默（记忆是增强项，不能拖垮主链路）。
    pub async fn consolidate(
        &self,
        session_id: &str,
        domain: &str,
        user_message: &str,
        final_answer: &str,
    ) -> usize {
        let sutra = match &self.sutra {
            Some(s) => s,
            None => return 0,
        };
        if session_id.is_empty() {
            return 0;
        }

        let facts = self.extract_facts(user_message, final_answer).await;
        if facts.is_empty() {
            return 0;
        }

        match sutra.auto_precipitate(session_id, domain, &facts).await {
            0 => 0,
            n => {
                tracing::info!(
                    "🧠 记忆沉淀: session={} domain={} 新增 {} 条",
                    session_id,
                    domain,
                    n
                );
                n
            }
        }
    }

    /// 两级提炼：先 LLM，后规则兜底
    async fn extract_facts(&self, user_message: &str, final_answer: &str) -> Vec<String> {
        let mut facts: Vec<String> = Vec::new();

        // ① LLM 提炼（对话有一定信息量才值得花这次调用）
        if user_message.chars().count() >= 8 {
            if let Some(llm) = &self.llm {
                if let Some(extracted) = Self::llm_extract(llm, user_message, final_answer).await {
                    facts.extend(extracted);
                }
            }
        }

        // ② 规则兜底：LLM 缺失/失败/没提炼出东西时补位
        if facts.is_empty() {
            facts.extend(Self::rule_extract(user_message));
        }

        // ③ 去噪 + 去重 + 限长
        //
        // 关键一步是 `grounded_in`：事实必须在**用户原话**里有依据。
        // 实测中模型会把助手回答里的通用知识（"Cycles 适合高质量渲染"）
        // 甚至用户的问题本身当成事实输出 —— 这些都不是"关于用户"的记忆，
        // 存进去只会稀释命中率。
        let mut seen = std::collections::HashSet::new();
        facts
            .into_iter()
            .map(|f| f.trim().to_string())
            .filter(|f| Self::is_worth_remembering(f))
            .filter(|f| Self::grounded_in(f, user_message))
            .filter(|f| seen.insert(f.clone()))
            .take(5)
            .collect()
    }

    /// 事实是否在用户原话中有依据（字面 2-gram 重叠率 ≥ 20%）
    ///
    /// 用字面重叠而非语义判断：零依赖、零成本，且对中英混排稳健。
    /// 阈值取 20% 是因为 LLM 会做同义改写（"渲染器用 Cycles" vs 原话
    /// "使用 Cycles 渲染器"），完全逐字比对会误杀。
    fn grounded_in(fact: &str, user_message: &str) -> bool {
        if user_message.trim().is_empty() {
            return false;
        }
        let fact_chars: Vec<char> = fact.chars().collect();
        if fact_chars.len() < 2 {
            return false;
        }
        let msg: String = user_message.chars().collect();
        let grams: Vec<String> = fact_chars
            .windows(2)
            .map(|w| w.iter().collect::<String>())
            .collect();
        if grams.is_empty() {
            return false;
        }
        let hit = grams.iter().filter(|g| msg.contains(g.as_str())).count();
        (hit as f32 / grams.len() as f32) >= 0.2
    }

    /// LLM 提炼：要求模型只输出 JSON 字符串数组
    async fn llm_extract(
        llm: &Arc<dyn subhuti_core::LLM>,
        user_message: &str,
        final_answer: &str,
    ) -> Option<Vec<String>> {
        let answer_excerpt: String = final_answer.chars().take(600).collect();
        let prompt = format!(
            "你是记忆提炼器。从【用户消息】中，提取值得**长期记住**的、关于这位用户的事实，\
             例如：他的偏好、固定参数、环境配置、项目约定、软硬件版本、明确目标。\n\n\
             严格要求：\n\
             1. 只输出 JSON 字符串数组，如 [\"用户渲染器用 Cycles\",\"采样值固定 128\"]\n\
             2. 每条不超过 60 字，语义完整可独立理解（写\"用户…\"，不要写\"他/她\"）\n\
             3. **只提炼用户自己说出来的信息**。助手回答里的通用知识、科普内容一律不要\n\
             4. **不要记录用户的提问**（如\"用户问了渲染器是多少\"），只记录陈述性事实\n\
             5. 最多 5 条；确实没有值得记住的事实就输出 []\n\
             6. 不要输出任何解释文字\n\n\
             【用户消息】\n{}\n\n\
             【助手回答】（仅供判断上下文，不要从中提炼事实）\n{}",
            user_message, answer_excerpt
        );

        let messages = vec![subhuti_core::runtime::llm::Message {
            role: subhuti_core::runtime::llm::Role::User,
            content: prompt,
            tool_call_id: None,
        }];

        match llm.chat(messages).await {
            Ok(raw) => Some(Self::parse_json_array(&raw)),
            Err(e) => {
                tracing::debug!("记忆提炼 LLM 调用失败，降级规则抽取: {}", e);
                None
            }
        }
    }

    /// 从模型输出里抠出 JSON 数组（容忍 ```json 包裹与前后废话）
    fn parse_json_array(raw: &str) -> Vec<String> {
        let trimmed = raw.trim();
        // 去掉 markdown 代码围栏
        let cleaned = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        // 截取第一个 [ 到最后一个 ]
        let (start, end) = match (cleaned.find('['), cleaned.rfind(']')) {
            (Some(s), Some(e)) if s < e => (s, e + 1),
            _ => return Vec::new(),
        };
        match serde_json::from_str::<serde_json::Value>(&cleaned[start..end]) {
            Ok(serde_json::Value::Array(arr)) => arr
                .into_iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// 规则兜底：抓"用户在陈述自己的情况/偏好/配置"的句子
    fn rule_extract(user_message: &str) -> Vec<String> {
        // 陈述性标记：出现这些词，说明用户在交代可复用的事实
        const MARKERS: &[&str] = &[
            "我用",
            "我用的是",
            "我使用",
            "我在用",
            "我偏好",
            "我喜欢",
            "我希望",
            "我想要",
            "我的是",
            "我的",
            "我这边",
            "我们项目",
            "我们团队",
            "记住",
            "请记住",
            "注意",
            "配置是",
            "设置为",
            "设为",
            "版本是",
            "安装的是",
            "固定",
            "默认",
        ];

        // 按中英文标点切句
        let mut out = Vec::new();
        for seg in user_message
            .split(|c: char| matches!(c, '。' | '！' | '？' | '!' | '?' | '\n' | '；' | ';'))
        {
            let s = seg.trim();
            if s.is_empty() {
                continue;
            }
            let hit = MARKERS.iter().any(|m| s.contains(m));
            let has_number = s.chars().any(|c| c.is_ascii_digit());
            // 有标记词，或含数字（参数/版本/采样值这类硬事实）
            if hit || has_number {
                out.push(s.to_string());
            }
        }
        out
    }

    /// 去噪：过滤掉没有长期价值的句子
    ///
    /// `pub(crate)` 是为了让 MCP 的手动写入入口（`adapter::inbound::mcp`）
    /// 复用同一套判据——手动 write 与自动沉淀是两条独立入口，判据必须同源，
    /// 否则「关注…/需要了解…」这类句子能从手动入口漏进库里。
    pub(crate) fn is_worth_remembering(fact: &str) -> bool {
        let len = fact.chars().count();
        if !(4..=200).contains(&len) {
            return false;
        }
        // 纯寒暄 / 无信息量的话
        const NOISE: &[&str] = &[
            "你好", "您好", "谢谢", "感谢", "在吗", "hi", "hello", "嗯嗯", "好的", "ok", "可以",
            "没事", "拜拜",
        ];
        let lower = fact.to_lowercase();
        if NOISE.iter().any(|n| lower.trim() == *n) {
            return false;
        }
        // 纯疑问句（问句不是事实）
        if fact.ends_with('？') || fact.ends_with('?') {
            return false;
        }
        // 提问意图句：描述"用户想知道什么"，而不是"用户是什么样"
        if Self::is_intent_sentence(fact) {
            return false;
        }
        true
    }

    /// 是否是「提问意图」或「空泛话题」而非关于用户的事实
    ///
    /// 实测中模型会把用户的提问意图当成事实沉淀，库里的典型样本：
    /// 「关注 Rust 模块在项目中的角色」「需要了解 Rust 模块的具体职责」
    /// 「希望获取 Rust 模块职责的详细信息」「对 Rust 模块职责有明确需求」。
    /// 这类句子不含任何关于用户的稳定信息，却会占满检索结果
    /// （实测一次 7 条结果里 4 条都是它），必须拦在落库之前。
    ///
    /// 六段判定（按命中成本从低到高）：
    /// - ① **句首**意图动词：用 `starts_with`，避免误杀「用户希望…」
    ///   （以"用户"开头，是事实而非意图）；
    /// - ② **句中**意图组合词：覆盖「我 + 希望 + 了解」这种被改写过的形态；
    /// - ③ **「用户/我 + 泛意图动词」**：模型常把提问改写成第三人称复述，
    ///   实测落库的有「用户关注 Rust 模块职责」「用户了解 Rust 编程」
    ///   「用户提及项目代码和模块」「用户需要查看 Blender 动画」；
    /// - ④ **话题标题式**：名词短语 + 介绍/描述/说明/应用/作用，
    ///   如「Blender 修改器介绍」「布尔运算在Blender中的应用」——
    ///   这是"聊到了什么"，不是"用户是什么样"；
    /// - ⑤ **通用知识句**：无用户锚点 + 「支持/用于/包括/分为/属于…」这类
    ///   能力描述谓语，如「Rust 模块支持抽象和复用功能」「Rust 模块用于组织
    ///   代码和封装」——这是**助手回答里的通用知识**，不是用户事实。
    ///   实测它与用户提问共享「Rust 模块」等 n-gram，能骗过 `grounded_in`
    ///   的 20% 字面重叠门槛，只能在这里拦。
    fn is_intent_sentence(fact: &str) -> bool {
        const PREFIXES: &[&str] = &[
            "关注",
            "需要",
            "希望",
            "想要",
            "想了解",
            "想知道",
            "询问",
            "请问",
            "求助",
            "帮忙",
            "如何",
            "怎么",
            "为什么",
            "是否",
            "能否",
            "求",
            "了解",
            "探索",
            "学习",
            "研究",
            "查看",
        ];
        if PREFIXES.iter().any(|p| fact.starts_with(p)) {
            return true;
        }

        const PHRASES: &[&str] = &[
            "需要了解",
            "希望了解",
            "希望获取",
            "希望知道",
            "想了解",
            "想知道",
            "有明确需求",
            "明确需求",
            "提了个问题",
            "提出疑问",
            "询问",
            "寻求",
        ];
        if PHRASES.iter().any(|p| fact.contains(p)) {
            return true;
        }

        if fact.starts_with("用户") || fact.starts_with("我") {
            const USER_VERBS: &[&str] = &[
                "关注",
                "了解",
                "提及",
                "提到",
                "询问",
                "想知道",
                "需要查看",
                "需要了解",
                "需要获取",
                "有具体需求",
                "有明确需求",
                "希望了解",
                "希望知道",
                "希望获取",
                "希望查看",
            ];
            if USER_VERBS.iter().any(|v| fact.contains(v)) {
                return true;
            }
        }

        // 话题标题式：只在短句上判定，避免误伤长正文式事实
        if fact.chars().count() <= 25 {
            const TOPIC_TAILS: &[&str] = &["介绍", "描述", "说明", "应用", "作用", "概述"];
            // 例外：**尾巴之前**出现系动词/动作词 → 它是陈述句，不是话题标题。
            // 实测误杀样本「提交信息用中文描述」：尾是「描述」，「用」在其之前。
            // ⚠️ 豁免必须**只看尾巴之前**：尾巴「应用」「作用」自身就含「用」，
            // 若对整句 `contains("用")` 会把这俩垃圾一并放过（自检会立刻报错）。
            const TAIL_EXEMPT: &[&str] = &["用", "是", "为", "必须"];
            for &t in TOPIC_TAILS {
                if let Some(stem) = fact.strip_suffix(t) {
                    if !TAIL_EXEMPT.iter().any(|w| stem.contains(w)) {
                        return true;
                    }
                }
            }
        }

        // ⑤ 通用知识句：无用户锚点 + 能力描述谓语。
        // 先看锚点——带锚点的句子（"本项目支持多租户"）是真事实，直接放过。
        // 注意：锚点里**不能**出现裸「项目」——「项目涉及 Rust 编程」这类泛话题句
        // 正是靠它逃逸的（2026-09-14 实测落库）。只认「本项目」这种明确指代；
        // 而「项目上下文」这类真事实不含能力谓语，删掉锚点后依然能通过。
        const ANCHORS: &[&str] = &[
            "用户",
            "我",
            "本项目",
            "我们",
            "偏好",
            "默认",
            "固定",
            "配置",
            "约定",
            "记住",
            "注意",
        ];
        if !ANCHORS.iter().any(|a| fact.contains(a)) && fact.chars().count() <= 25 {
            const GENERIC_PREDICATES: &[&str] = &[
                "支持",
                "用于",
                "包括",
                "分为",
                "可分为",
                "属于",
                "指的是",
                "是指",
                "之分",
                "组成",
                "涉及",
            ];
            if GENERIC_PREDICATES.iter().any(|p| fact.contains(p)) {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_array_from_fenced_json() {
        let raw = "```json\n[\"用户使用 Cycles\", \"采样 128\"]\n```";
        let out = MemoryConsolidator::parse_json_array(raw);
        assert_eq!(out, vec!["用户使用 Cycles", "采样 128"]);
    }

    #[test]
    fn parse_array_tolerates_surrounding_text() {
        let raw = "好的，结果是 [\"渲染器是 Cycles\"] 就这样";
        let out = MemoryConsolidator::parse_json_array(raw);
        assert_eq!(out, vec!["渲染器是 Cycles"]);
    }

    #[test]
    fn parse_array_returns_empty_on_garbage() {
        assert!(MemoryConsolidator::parse_json_array("没有事实").is_empty());
    }

    #[test]
    fn rule_extract_catches_preference_sentences() {
        let facts =
            MemoryConsolidator::rule_extract("我用 Cycles 渲染器。今天天气不错。采样值设为 128");
        assert!(facts.iter().any(|f| f.contains("Cycles")));
        assert!(facts.iter().any(|f| f.contains("128")));
        // 无标记且无数字的句子不该被抽出
        assert!(!facts.iter().any(|f| f.contains("天气")));
    }

    #[test]
    fn noise_is_filtered_out() {
        assert!(!MemoryConsolidator::is_worth_remembering("你好"));
        assert!(!MemoryConsolidator::is_worth_remembering("谢谢"));
        assert!(!MemoryConsolidator::is_worth_remembering("这是什么？"));
        assert!(!MemoryConsolidator::is_worth_remembering("嗯"));
        assert!(MemoryConsolidator::is_worth_remembering(
            "用户渲染器使用 Cycles"
        ));
    }

    /// 回归：实测中真的落进库里的「提问意图 / 空泛话题」垃圾句必须被拦住
    #[test]
    fn intent_sentences_are_rejected() {
        for junk in [
            // 第一轮实测落库（句首 / 句中意图词）
            "关注 Rust 模块在项目中的角色",
            "需要了解 Rust 模块的具体职责",
            "希望获取 Rust 模块职责的详细信息",
            "对 Rust 模块职责有明确需求",
            "我希望了解 rust 模块的职责",
            // 第二轮实测落库（第三人称复述 / 话题标题式）
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
            // 第三轮实测落库（通用知识句 / 动作片段）
            "Rust 模块支持抽象和复用功能",
            "Rust 模块支持模块化设计",
            "Rust 模块有顶层、嵌套和私有之分",
            "Rust 模块用于组织代码和封装",
            "了解几何修改器",
            "探索修改器类型",
            // 第四轮实测落库（泛话题 + 弱谓语「涉及」；锚点表曾含裸「项目」而被放过）
            "项目涉及 Rust 编程",
        ] {
            assert!(
                !MemoryConsolidator::is_worth_remembering(junk),
                "提问意图/空泛话题句应被过滤: {junk}"
            );
        }
    }

    /// 真实事实不能被意图过滤误伤
    #[test]
    fn real_facts_survive_intent_filter() {
        for fact in [
            "用户渲染器用 Cycles",
            "采样值固定 128",
            "用户偏好：提交前必须跑 cargo fmt 与 cargo clippy",
            "输出格式统一 PNG",
            "用户希望默认走 20 积分路径，不加 PBR",
            "Rust 基础知识与最佳实践",
            "Rust 设计模式",
            "个人编码约定",
            "项目上下文",
            // 带用户锚点的能力描述是真事实，不能被 ⑤ 段误杀
            "本项目支持多租户",
            "用户项目支持插件扩展",
            // 「涉及」+ 锚点同样是真事实（无锚点的才拦）
            "用户的项目涉及支付与风控",
            "本项目涉及 Blender 与 Rust 两端",
            // ④ 段豁免：以话题尾收尾但含动作词 → 是陈述句而非话题标题
            "提交信息用中文描述",
        ] {
            assert!(
                MemoryConsolidator::is_worth_remembering(fact),
                "真实事实不应被过滤: {fact}"
            );
        }
    }
}
