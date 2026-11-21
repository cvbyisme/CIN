//! 刷写计划构造。
//!
//! 设计原则：
//! - 这里**只构造命令**，不执行、不落盘、不判断工具链是否存在；
//! - 每个 plan 都必须是 `Risk::High` 并且至少带一条后果说明，
//!   因为刷写分区是整个工具里最不可逆的动作；
//! - 分区名与镜像路径要做基本校验，避免把用户的笔误变成 `fastboot flash boot boot`
//!   这类静默失败的调用。

use std::path::Path;

use crate::confirm::{Plan, Risk};
use crate::reboot::Protocol;

/// 单个分区的刷写任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashTask {
    /// 分区名，例如 `boot`、`vbmeta`、`system`。
    pub partition: String,
    /// 镜像的绝对路径。
    pub image: String,
}

impl FlashTask {
    pub fn new(partition: impl Into<String>, image: impl Into<String>) -> Self {
        Self {
            partition: partition.into(),
            image: image.into(),
        }
    }

    /// 分区名是否看起来合法：只允许字母、数字、下划线、连字符。
    ///
    /// 这条规则挡住了空格、`;`、`$`、`/` 等会在 shell 里出问题的字符，
    /// 也挡住了把路径误填进分区名的情况。
    pub fn validate(&self) -> Result<(), String> {
        if self.partition.is_empty() {
            return Err("分区名为空".to_string());
        }
        let ok = self
            .partition
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !ok {
            return Err(format!(
                "分区名 {:?} 含非法字符（只允许字母、数字、下划线和连字符）",
                self.partition
            ));
        }
        if self.image.trim().is_empty() {
            return Err(format!("分区 {} 的镜像路径为空", self.partition));
        }
        if !Path::new(&self.image).is_absolute() {
            return Err(format!(
                "镜像路径必须是绝对路径：{:?}（fastboot 在设备侧解析路径，相对路径会失败）",
                self.image
            ));
        }
        Ok(())
    }

    /// 生成 fastboot argv。
    pub fn argv(&self, serial: &str) -> Vec<String> {
        vec![
            "fastboot".to_string(),
            "-s".to_string(),
            serial.to_string(),
            "flash".to_string(),
            self.partition.clone(),
            self.image.clone(),
        ]
    }
}

/// 一次刷写动作的完整计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashPlan {
    pub tasks: Vec<FlashTask>,
    /// 刷完后是否自动重启。
    pub reboot_after: bool,
}

impl FlashPlan {
    pub fn new(tasks: Vec<FlashTask>) -> Self {
        Self {
            tasks,
            reboot_after: false,
        }
    }

    pub fn with_reboot(mut self) -> Self {
        self.reboot_after = true;
        self
    }

    /// 校验所有任务，返回第一个错误。
    pub fn validate(&self) -> Result<(), String> {
        if self.tasks.is_empty() {
            return Err("没有要刷写的分区".to_string());
        }
        // 同一个分区刷两次通常是笔误，明确报错比默默覆盖好
        for (i, t) in self.tasks.iter().enumerate() {
            t.validate()?;
            for prev in &self.tasks[..i] {
                if prev.partition == t.partition {
                    return Err(format!("分区 {} 被指定了两次", t.partition));
                }
            }
        }
        Ok(())
    }

    /// 生成带确认信息的 `Plan`。
    ///
    /// 注意：这里只支持 fastboot 协议。adb 模式下刷分区走 `adb sideload`
    /// 或 `adb push` + root shell，语义完全不同，不适合混在同一个 plan 里。
    pub fn to_plan(&self, protocol: Protocol, serial: &str) -> Result<Plan, String> {
        self.validate()?;
        if protocol != Protocol::Fastboot {
            return Err("刷写分区需要设备处于 fastboot 模式".to_string());
        }
        if serial.trim().is_empty() {
            return Err("缺少设备序列号".to_string());
        }

        let summary = if self.tasks.len() == 1 {
            format!("刷写设备 {} 的 {} 分区", serial, self.tasks[0].partition)
        } else {
            format!(
                "刷写设备 {} 的 {} 个分区（{}）",
                serial,
                self.tasks.len(),
                self.tasks
                    .iter()
                    .map(|t| t.partition.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        let mut plan = Plan::new(summary, Risk::High);
        for task in &self.tasks {
            plan = plan.command(task.argv(serial));
        }
        for task in &self.tasks {
            plan = plan.consequence(format!(
                "分区 {} 的内容将被镜像覆盖，原数据无法恢复",
                task.partition
            ));
        }
        plan = plan.consequence("刷写错误的分区可能导致设备无法开机");
        if self.reboot_after {
            plan = plan.command(vec![
                "fastboot".to_string(),
                "-s".to_string(),
                serial.to_string(),
                "reboot".to_string(),
            ]);
            plan = plan.consequence("刷写完成后设备会自动重启");
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(p: &str, i: &str) -> FlashTask {
        FlashTask::new(p, i)
    }

    #[test]
    fn single_partition_plan_is_high_risk() {
        let fp = FlashPlan::new(vec![task("boot", "/sdcard/boot.img")]);
        let plan = fp.to_plan(Protocol::Fastboot, "ABC123").unwrap();
        assert_eq!(plan.risk, Risk::High);
        assert_eq!(plan.commands.len(), 1);
        assert_eq!(
            plan.commands[0],
            vec![
                "fastboot",
                "-s",
                "ABC123",
                "flash",
                "boot",
                "/sdcard/boot.img"
            ]
        );
        // 确认文本必须包含确切命令
        let text = plan.render();
        assert!(text.contains("$ fastboot -s ABC123 flash boot /sdcard/boot.img"));
    }

    #[test]
    fn multiple_partitions_are_ordered() {
        let fp = FlashPlan::new(vec![
            task("vbmeta", "/sdcard/vbmeta.img"),
            task("boot", "/sdcard/boot.img"),
        ]);
        let plan = fp.to_plan(Protocol::Fastboot, "X").unwrap();
        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[0][4], "vbmeta");
        assert_eq!(plan.commands[1][4], "boot");
        assert!(plan.summary.contains("2 个分区"));
    }

    #[test]
    fn reboot_after_appends_reboot_command() {
        let fp = FlashPlan::new(vec![task("boot", "/sdcard/boot.img")]).with_reboot();
        let plan = fp.to_plan(Protocol::Fastboot, "X").unwrap();
        assert_eq!(plan.commands.len(), 2);
        assert_eq!(plan.commands[1], vec!["fastboot", "-s", "X", "reboot"]);
        assert!(plan.consequences.iter().any(|c| c.contains("自动重启")));
    }

    #[test]
    fn adb_protocol_is_rejected() {
        let fp = FlashPlan::new(vec![task("boot", "/sdcard/boot.img")]);
        let err = fp.to_plan(Protocol::Adb, "X").unwrap_err();
        assert!(err.contains("fastboot"), "err={err}");
    }

    #[test]
    fn illegal_partition_name_is_rejected() {
        for bad in ["boot;rm -rf /", "boot img", "../boot", ""] {
            let t = task(bad, "/sdcard/x.img");
            assert!(t.validate().is_err(), "分区名 {bad:?} 应被拒绝");
        }
        for good in ["boot", "vbmeta_system", "dtbo-a", "0"] {
            let t = task(good, "/sdcard/x.img");
            assert!(t.validate().is_ok(), "分区名 {good:?} 应被接受");
        }
    }

    #[test]
    fn relative_image_path_is_rejected() {
        let t = task("boot", "boot.img");
        let err = t.validate().unwrap_err();
        assert!(err.contains("绝对路径"), "err={err}");
    }

    #[test]
    fn duplicate_partition_is_rejected() {
        let fp = FlashPlan::new(vec![
            task("boot", "/sdcard/a.img"),
            task("boot", "/sdcard/b.img"),
        ]);
        let err = fp.validate().unwrap_err();
        assert!(err.contains("两次"), "err={err}");
    }

    #[test]
    fn empty_plan_is_rejected() {
        let fp = FlashPlan::new(vec![]);
        assert!(fp.to_plan(Protocol::Fastboot, "X").is_err());
    }

    #[test]
    fn empty_serial_is_rejected() {
        let fp = FlashPlan::new(vec![task("boot", "/sdcard/boot.img")]);
        assert!(fp.to_plan(Protocol::Fastboot, "  ").is_err());
    }

    #[test]
    fn every_flash_plan_has_consequences() {
        let fp = FlashPlan::new(vec![task("boot", "/sdcard/boot.img")]);
        let plan = fp.to_plan(Protocol::Fastboot, "X").unwrap();
        assert!(!plan.consequences.is_empty());
        assert!(plan.consequences.iter().any(|c| c.contains("无法恢复")));
    }
}
