//! 破坏性操作的确认。
//!
//! 设计取舍：
//! - 确认逻辑本身是纯函数（[`Decision`]），谁负责读写 stdin 由调用方决定；
//! - 提示里必须原样列出**将要执行的确切命令**，而不是"确定要刷机吗"这种模糊问句；
//! - 默认答案永远是「否」，回车即取消。

use std::fmt;

/// 风险等级。决定提示的措辞与是否强制要求输入额外凭据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// 只读，不需要确认。
    None,
    /// 可恢复的修改（例如重启到 fastboot）。默认仍需确认，但可用 `--yes` 跳过。
    Low,
    /// 不可逆：刷写分区、清除数据、擦除。`--yes` 可以跳过确认，但提示措辞会更重。
    High,
}

impl fmt::Display for Risk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Risk::None => write!(f, "只读"),
            Risk::Low => write!(f, "可恢复"),
            Risk::High => write!(f, "不可逆"),
        }
    }
}

/// 一次操作的完整描述，用于生成确认提示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// 一句话说清要做什么。
    pub summary: String,
    /// 将要执行的确切 argv，按执行顺序。
    pub commands: Vec<Vec<String>>,
    pub risk: Risk,
    /// 额外的后果说明，逐行展示。
    pub consequences: Vec<String>,
}

impl Plan {
    pub fn read_only(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            commands: Vec::new(),
            risk: Risk::None,
            consequences: Vec::new(),
        }
    }

    pub fn new(summary: impl Into<String>, risk: Risk) -> Self {
        Self {
            summary: summary.into(),
            commands: Vec::new(),
            risk,
            consequences: Vec::new(),
        }
    }

    pub fn command(mut self, argv: Vec<String>) -> Self {
        self.commands.push(argv);
        self
    }

    pub fn consequence(mut self, text: impl Into<String>) -> Self {
        self.consequences.push(text.into());
        self
    }

    /// 生成给用户看的完整文本（不含最后的 y/N 问句）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\n", self.summary));
        out.push_str(&format!("风险等级: {}\n", self.risk));

        if !self.consequences.is_empty() {
            out.push('\n');
            for c in &self.consequences {
                out.push_str(&format!("  ! {}\n", c));
            }
        }

        if !self.commands.is_empty() {
            out.push_str("\n将执行:\n");
            for argv in &self.commands {
                out.push_str(&format!("  $ {}\n", shell_join(argv)));
            }
        }
        out
    }
}

/// 与 POSIX shell 一致的参数引用，保证显示出来的命令可以直接复制去跑。
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+".contains(c));
    if safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// 用户的回答。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Proceed,
    Abort,
}

/// 把一行输入解析成决定。只接受明确的肯定词，其余一律视为取消。
pub fn parse_answer(input: &str) -> Decision {
    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => Decision::Proceed,
        _ => Decision::Abort,
    }
}

/// 需要确认的最终判定：`--yes` 能跳过风险提示，但跳不过 `Risk::High` 下未显式声明的场景。
///
/// 这里的规则很简单：
/// - `Risk::None` → 直接放行；
/// - 用户传了 `--yes` → 放行；
/// - 否则 → 需要确认。
pub fn needs_prompt(risk: Risk, assume_yes: bool) -> bool {
    match risk {
        Risk::None => false,
        _ => !assume_yes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn read_only_plan_never_prompts() {
        assert!(!needs_prompt(Risk::None, false));
        assert!(!needs_prompt(Risk::None, true));
    }

    #[test]
    fn low_and_high_risk_prompt_without_yes() {
        assert!(needs_prompt(Risk::Low, false));
        assert!(needs_prompt(Risk::High, false));
        assert!(!needs_prompt(Risk::Low, true));
        assert!(!needs_prompt(Risk::High, true));
    }

    #[test]
    fn only_explicit_yes_proceeds() {
        for yes in ["y", "Y", "yes", "YES", " yes "] {
            assert_eq!(parse_answer(yes), Decision::Proceed, "input={yes:?}");
        }
        for no in ["", "n", "no", "ok", "\n", "yy"] {
            assert_eq!(parse_answer(no), Decision::Abort, "input={no:?}");
        }
    }

    #[test]
    fn render_lists_exact_commands() {
        let plan = Plan::new("刷写 boot 分区", Risk::High)
            .command(argv(&["fastboot", "flash", "boot", "/sdcard/boot.img"]))
            .consequence("设备将无法开机，除非镜像正确");
        let text = plan.render();
        assert!(text.contains("刷写 boot 分区"));
        assert!(text.contains("不可逆"));
        assert!(text.contains("$ fastboot flash boot /sdcard/boot.img"));
        assert!(text.contains("! 设备将无法开机"));
    }

    #[test]
    fn shell_join_quotes_when_needed() {
        assert_eq!(
            shell_join(&argv(&["fastboot", "flash", "boot", "/a/b.img"])),
            "fastboot flash boot /a/b.img"
        );
        assert_eq!(
            shell_join(&argv(&["echo", "hello world"])),
            "echo 'hello world'"
        );
        assert_eq!(shell_join(&argv(&["echo", "it's"])), r"echo 'it'\''s'");
        assert_eq!(shell_join(&argv(&["echo", ""])), "echo ''");
    }

    #[test]
    fn render_without_commands_still_works() {
        let plan = Plan::read_only("列出设备");
        let text = plan.render();
        assert!(text.contains("只读"));
        assert!(!text.contains("将执行"));
    }
}
