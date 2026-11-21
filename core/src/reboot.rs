//! 重启目标 → 实际命令。
//!
//! 关键认知：adb 与 fastboot 是两套协议，同一个目标在不同模式下命令不同。
//! 映射集中在这里，避免调用方各写一份互相矛盾。

use crate::confirm::{Plan, Risk};

/// 重启目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// 重启进系统。
    System,
    /// 重启到 bootloader。
    Bootloader,
    /// 重启到 fastbootd（动态分区操作需要）。
    Fastbootd,
    /// 重启到 recovery。
    Recovery,
    /// 重启到 EDL / 9008（仅部分高通机型）。
    Edl,
}

impl Target {
    /// 解析用户输入，容忍常见别名。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "system" | "sys" | "android" => Some(Target::System),
            "bootloader" | "bl" | "boot" => Some(Target::Bootloader),
            "fastbootd" | "fbd" | "fastboot" => Some(Target::Fastbootd),
            "recovery" | "rec" => Some(Target::Recovery),
            "edl" | "9008" | "qdl" => Some(Target::Edl),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Target::System => "system",
            Target::Bootloader => "bootloader",
            Target::Fastbootd => "fastbootd",
            Target::Recovery => "recovery",
            Target::Edl => "edl",
        }
    }

    /// 已知目标清单，用于错误提示。
    pub fn all() -> &'static [Target] {
        &[
            Target::System,
            Target::Bootloader,
            Target::Fastbootd,
            Target::Recovery,
            Target::Edl,
        ]
    }

    /// 各目标的额外后果说明。
    pub fn consequences(self) -> Vec<String> {
        match self {
            Target::System => vec!["设备将重启，当前所有未保存的工作会丢失".into()],
            Target::Bootloader => vec!["设备将进入 bootloader，屏幕显示 FASTBOOT 字样".into()],
            Target::Fastbootd => vec!["设备将进入用户空间 fastboot，仅支持动态分区相关操作".into()],
            Target::Recovery => vec!["设备将进入 recovery 界面".into()],
            Target::Edl => vec![
                "设备将进入 9008 / EDL 模式，此模式下系统完全不可用".into(),
                "不是所有机型都支持 `adb reboot edl`".into(),
            ],
        }
    }
}

/// 设备当前处于哪种协议，决定用哪套命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Adb,
    Fastboot,
}

/// 生成重启计划。
pub fn plan(target: Target, protocol: Protocol, serial: &str) -> Plan {
    let argv = match (protocol, target) {
        // fastboot 侧只有 reboot，没有 "reboot system" 这种说法
        (Protocol::Fastboot, Target::System) => vec!["fastboot", "-s", serial, "reboot"],
        (Protocol::Fastboot, Target::Bootloader) => {
            vec!["fastboot", "-s", serial, "reboot-bootloader"]
        }
        (Protocol::Fastboot, Target::Fastbootd) => {
            vec!["fastboot", "-s", serial, "reboot", "fastboot"]
        }
        (Protocol::Fastboot, Target::Recovery) => {
            vec!["fastboot", "-s", serial, "reboot", "recovery"]
        }
        (Protocol::Fastboot, Target::Edl) => {
            // 新版 fastboot 用 oem edl，没有统一标准，失败会明确报错
            vec!["fastboot", "-s", serial, "oem", "edl"]
        }
        (Protocol::Adb, _) => vec!["adb", "-s", serial, "reboot", target.as_str()],
    };

    Plan::new(
        format!("重启设备 {} 到 {}", serial, target.as_str()),
        Risk::Low,
    )
    .command(argv.into_iter().map(String::from).collect())
    .tap(|p| {
        for c in target.consequences() {
            p.consequences.push(c);
        }
    })
}

/// 小工具：允许在链式构造里就地修改。
trait Tap: Sized {
    fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}
impl Tap for Plan {}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(plan: &Plan) -> Vec<String> {
        plan.commands.first().cloned().unwrap_or_default()
    }

    #[test]
    fn parse_accepts_aliases() {
        assert_eq!(Target::parse("bootloader"), Some(Target::Bootloader));
        assert_eq!(Target::parse("BL"), Some(Target::Bootloader));
        assert_eq!(Target::parse("fastboot"), Some(Target::Fastbootd));
        assert_eq!(Target::parse("9008"), Some(Target::Edl));
        assert_eq!(Target::parse("recovery"), Some(Target::Recovery));
        assert_eq!(Target::parse("nonsense"), None);
    }

    #[test]
    fn adb_uses_reboot_subcommand() {
        let p = plan(Target::Recovery, Protocol::Adb, "ABC123");
        assert_eq!(argv(&p), vec!["adb", "-s", "ABC123", "reboot", "recovery"]);
        assert_eq!(p.risk, Risk::Low);
    }

    #[test]
    fn fastboot_system_is_plain_reboot() {
        let p = plan(Target::System, Protocol::Fastboot, "ABC123");
        assert_eq!(argv(&p), vec!["fastboot", "-s", "ABC123", "reboot"]);
    }

    #[test]
    fn fastboot_bootloader_uses_dedicated_flag() {
        let p = plan(Target::Bootloader, Protocol::Fastboot, "ABC123");
        assert_eq!(
            argv(&p),
            vec!["fastboot", "-s", "ABC123", "reboot-bootloader"]
        );
    }

    #[test]
    fn fastbootd_is_a_reboot_argument() {
        let p = plan(Target::Fastbootd, Protocol::Fastboot, "ABC123");
        assert_eq!(
            argv(&p),
            vec!["fastboot", "-s", "ABC123", "reboot", "fastboot"]
        );
    }

    #[test]
    fn edl_on_fastboot_uses_oem_edl() {
        let p = plan(Target::Edl, Protocol::Fastboot, "ABC123");
        assert_eq!(argv(&p), vec!["fastboot", "-s", "ABC123", "oem", "edl"]);
        assert!(p.consequences.iter().any(|c| c.contains("9008")));
    }

    #[test]
    fn every_target_has_a_consequence_note() {
        for t in Target::all() {
            assert!(!t.consequences().is_empty(), "目标 {:?} 缺少后果说明", t);
        }
    }

    #[test]
    fn serial_is_always_pinned() {
        for t in Target::all() {
            for proto in [Protocol::Adb, Protocol::Fastboot] {
                let a = argv(&plan(*t, proto, "SERIAL"));
                assert!(
                    a.iter().any(|x| x == "SERIAL"),
                    "{:?}/{:?} 未指定 -s 序列号",
                    t,
                    proto
                );
            }
        }
    }
}
