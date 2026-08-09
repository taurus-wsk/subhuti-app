use subhuti_plugin_sdk::{export_expert_plugin, ExpertPlugin, PluginManifest};

pub struct PsychologyExpert;

impl ExpertPlugin for PsychologyExpert {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "psychology-wasm".to_string(),
            name: "心理咨询专家 (WASM)".to_string(),
            description: "专业的心理咨询专家，擅长情绪疏导、压力管理、人际关系和自我成长"
                .to_string(),
            version: "0.1.0".to_string(),
        }
    }

    fn run(input: &str) -> String {
        let input_lower = input.to_lowercase();

        if check_crisis(&input_lower) {
            return "我注意到你可能正在经历非常困难的时刻。\
                如果你有任何关于自我伤害的想法，我真的很担心你。\
                请你现在联系身边信任的人，或者拨打心理危机干预热线：\
                全国心理援助热线：400-161-9995\
                北京心理危机研究与干预中心：010-82951332\
                如果你有紧急危险，请立即拨打120或110。\
                你不是一个人，有人关心你，也有人可以帮助你。"
                .to_string();
        }

        let mood_score = match_mood(&input_lower);
        let stress_score = match_stress(&input_lower);

        if mood_score > 0.5 {
            format!(
                "我感受到你可能正在经历一些情绪波动。这很正常，每个人都会有这样的时候。\
                你愿意多和我说说，是什么事情让你有这样的感受吗？\
                有时候，把心里的话说出来，本身就是一种疗愈。\n\n\
                用户输入：{}",
                input
            )
        } else if stress_score > 0.5 {
            format!(
                "压力是生活中很常见的体验，你并不孤单。让我分享一个简单的放松练习：\n\n\
                🌿 **4-7-8 呼吸法**\n\n\
                1. 用鼻子安静地吸气，心里数 4 秒\n\
                2. 屏住呼吸，数 7 秒\n\
                3. 用嘴巴慢慢呼气，数 8 秒\n\n\
                重复 3-4 次，你可能会感到平静一些。\n\n\
                用户输入：{}",
                input
            )
        } else {
            format!(
                "你好！我是暖心心理咨询师。我很乐意倾听你的故事，\
                帮助你探索内心世界，提供专业的心理支持和成长建议。\n\n\
                用户输入：{}",
                input
            )
        }
    }

    fn on_activate() -> Result<(), String> {
        Ok(())
    }

    fn on_deactivate() -> Result<(), String> {
        Ok(())
    }
}

fn check_crisis(input: &str) -> bool {
    let crisis_keywords = ["自杀", "不想活", "死了算了", "结束生命", "自残"];
    crisis_keywords.iter().any(|kw| input.contains(*kw))
}

fn match_mood(input: &str) -> f32 {
    let keywords = ["心情", "情绪", "不开心", "难过", "烦躁"];
    let score = keywords.iter().filter(|kw| input.contains(*kw)).count() as f32;
    score / keywords.len() as f32
}

fn match_stress(input: &str) -> f32 {
    let keywords = ["压力", "焦虑", "紧张", "放松", "减压", "失眠"];
    let score = keywords.iter().filter(|kw| input.contains(*kw)).count() as f32;
    score / keywords.len() as f32
}

export_expert_plugin!(PsychologyExpert);
