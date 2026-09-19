//! # 技能注册表收敛宏（Skill = Flow + Tool 的元数据登记）
//!
//! 领域专家过去每个都要手写一份 `XxxSkillKind` 枚举 + `ALL` /
//! `id/name/description/parameters` / 特有方法(`stages`/`system_prompt`) /
//! `from_id` / `meta` 的样板（~100 行）。本宏把这份**数据方法**收敛为一张
//! 单行注册表：新增技能/专家只改数据表一处，编译器在 `flow_pure`/`flow_llm`
//! 分派处仍能穷尽 match。
//!
//! 边界：宏**只收敛"数据方法"**，`flow_pure`/`flow_llm` 的阶段×技能分派
//! （真正业务逻辑）保持手写，绝不放进宏。不引入 proc-macro / 新 crate 依赖。

/// 变体计数助手（const 可求值）：`<[()]>::len(&[(), ...])` → 变体数量，供 `ALL` 数组长度使用。
#[macro_export]
macro_rules! skill_catalog_count {
    ($($v:ident),* $(,)?) => {
        <[()]>::len(&[$($crate::skill_catalog_count!(@unit $v)),*])
    };
    (@unit $v:ident) => { () };
}

/// 由一张技能数据表生成枚举 + 全部注册数据方法 + 一个"特有方法"。
///
/// 参数：
/// - `kind`：枚举名（如 `RustSkillKind`）。
/// - `extra_name`：特有方法名（rust=`stages`、blender=`system_prompt`）。
/// - `extra_ty`：该方法的返回类型（`&'static [ReactStage]` / `&'static str`）。
/// - `{ ... }`：数据表，每行 `Variant { id, name, desc, params:[...], custom }`，
///   其中 `custom` 是 `extra_name` 该方法对应当前变体的表达式。
///
/// 生成成员：`enum`（+`Debug/Clone/Copy/PartialEq/Eq` 派生）+ `ALL` +
/// `const fn id/name/description/parameters` + `fn <extra_name>` + `from_id` + `meta`。
#[macro_export]
macro_rules! skill_catalog {
    ($kind:ident, $extra_name:ident, $extra_ty:ty,
     { $($variant:ident { id: $id:expr, name: $name:expr, desc: $desc:expr,
         params: [$($p:literal),* $(,)?], custom: $custom:expr $(,)? },)* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $kind {
            $($variant,)*
        }

        impl $kind {
            const ALL: [$kind; $crate::skill_catalog_count!($($variant),*)] =
                [$(Self::$variant),*];

            const fn id(self) -> &'static str {
                match self { $(Self::$variant => $id,)* }
            }

            const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name,)* }
            }

            const fn description(self) -> &'static str {
                match self { $(Self::$variant => $desc,)* }
            }

            const fn parameters(self) -> &'static [&'static str] {
                match self { $(Self::$variant => &[$($p),*],)* }
            }

            fn $extra_name(self) -> $extra_ty {
                match self { $(Self::$variant => $custom,)* }
            }

            /// 由对外 id 反查技能种类（`execute_skill` 入口使用）
            fn from_id(id: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|k| k.id() == id)
            }

            /// 派生技能元数据（`skills()` 的唯一来源）
            fn meta(self) -> DomainSkill {
                DomainSkill {
                    id: self.id().to_string(),
                    name: self.name().to_string(),
                    description: self.description().to_string(),
                    parameters: self.parameters().iter().map(|s| s.to_string()).collect(),
                }
            }
        }
    };
}
