//! 设备发现与状态解析。
//!
//! 本模块只做「把工具输出翻译成结构体」，不做任何进程调用，
//! 因此可以对着大量真实日志做回归测试。

use std::collections::BTreeMap;

/// 设备所处的模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// adb 已授权，可执行 shell。
    Adb,
    /// 已连接但未在设备端确认授权。
    AdbUnauthorized,
    /// adb offline，一般需要重新插拔。
    AdbOffline,
    /// recovery 中的 sideload 模式。
    AdbSideload,
    /// bootloader。
    Fastboot,
    /// 用户空间 fastbootd，动态分区操作必须在此模式。
    Fastbootd,
    /// 高通 9008 / EDL。第一期只识别，不刷写。
    Edl,
    /// 未知或无法归类。
    Unknown,
}

impl Mode {
    pub fn is_fastboot(self) -> bool {
        matches!(self, Mode::Fastboot | Mode::Fastbootd)
    }

    pub fn is_adb_usable(self) -> bool {
        matches!(self, Mode::Adb | Mode::AdbSideload)
    }
}

/// 一台设备。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub mode: Mode,
    /// 原始状态字段，保留用于排查未知值。
    pub raw_state: String,
    /// adb 的 transport_id，fastboot 下为空。
    pub transport_id: Option<String>,
}

impl Device {
    pub fn is_fastboot(&self) -> bool {
        self.mode.is_fastboot()
    }

    pub fn is_adb_usable(&self) -> bool {
        self.mode.is_adb_usable()
    }
}

/// 把 `adb devices -l` 切成设备列表。
///
/// 容忍：
/// - 首行 `List of devices attached`
/// - `adb server is out of date` 之类的杂项提示
/// - 空行与 `* daemon started successfully *`
pub fn parse_adb_devices(output: &str) -> Vec<Device> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("List of devices"))
        .filter(|l| !l.starts_with('*'))
        .filter(|l| !l.starts_with("adb server"))
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let serial = it.next()?;
            let state = it.next()?;
            let transport_id = line
                .split_whitespace()
                .find_map(|f| f.strip_prefix("transport_id:"))
                .map(str::to_string);
            Some(Device {
                serial: serial.to_string(),
                mode: adb_state_to_mode(state),
                raw_state: state.to_string(),
                transport_id,
            })
        })
        .collect()
}

/// 把 `fastboot devices` 切成设备列表。
pub fn parse_fastboot_devices(output: &str) -> Vec<Device> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        // fastboot 在等待设备时会打印 "< waiting for any device >"，不能当成设备
        .filter(|l| !l.starts_with('<'))
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let serial = it.next()?;
            let state = it.next()?;
            let mode = match state.to_ascii_lowercase().as_str() {
                s if s.starts_with("fastbootd") => Mode::Fastbootd,
                s if s.starts_with("fastboot") => Mode::Fastboot,
                _ => Mode::Unknown,
            };
            Some(Device {
                serial: serial.to_string(),
                mode,
                raw_state: state.to_string(),
                transport_id: None,
            })
        })
        .collect()
}

/// 只看关心的几个变量，避免把 `getvar all` 的巨大输出塞进内存。
pub fn parse_getvar_all(output: &str) -> BTreeMap<String, String> {
    const WANTED: &[&str] = &[
        "product",
        "variant",
        "serialno",
        "secure",
        "unlocked",
        "current-slot",
        "slot-count",
        "max-download-size",
        "partition-type:super",
        "is-logical:super",
        "battery-voltage",
    ];

    output
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            // 形如 "(bootloader) product: o1s"
            let line = line.strip_prefix("(bootloader)").unwrap_or(line).trim();
            let (key, value) = line.split_once(':')?;
            let key = key.trim().to_ascii_lowercase();
            if WANTED.contains(&key.as_str()) {
                Some((key, value.trim().to_string()))
            } else {
                None
            }
        })
        .collect()
}

fn adb_state_to_mode(state: &str) -> Mode {
    match state {
        "device" => Mode::Adb,
        "unauthorized" => Mode::AdbUnauthorized,
        "offline" => Mode::AdbOffline,
        "sideload" => Mode::AdbSideload,
        "bootloader" => Mode::Fastboot,
        "recovery" => Mode::Adb,
        _ => Mode::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adb_devices_with_transport_id() {
        let out = "List of devices attached\nR58M1234\tdevice product:o1s transport_id:3\n";
        let d = parse_adb_devices(out);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].serial, "R58M1234");
        assert_eq!(d[0].mode, Mode::Adb);
        assert_eq!(d[0].transport_id.as_deref(), Some("3"));
        assert!(d[0].is_adb_usable());
    }

    #[test]
    fn adb_devices_noise_is_ignored() {
        let out =
            "* daemon started successfully *\nList of devices attached\n\nabc\tunauthorized\n";
        let d = parse_adb_devices(out);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].mode, Mode::AdbUnauthorized);
        assert!(!d[0].is_adb_usable());
    }

    #[test]
    fn adb_empty_output() {
        assert!(parse_adb_devices("List of devices attached\n").is_empty());
        assert!(parse_adb_devices("").is_empty());
    }

    #[test]
    fn adb_sideload_mode() {
        let d = parse_adb_devices("xyz\tsideload\n");
        assert_eq!(d[0].mode, Mode::AdbSideload);
        assert!(d[0].is_adb_usable());
    }

    #[test]
    fn fastboot_and_fastbootd_are_distinct() {
        let d = parse_fastboot_devices("abc\tfastboot\nxyz\tfastbootd\n");
        assert_eq!(d[0].mode, Mode::Fastboot);
        assert_eq!(d[1].mode, Mode::Fastbootd);
        assert!(d.iter().all(Device::is_fastboot));
    }

    #[test]
    fn fastboot_ignores_waiting_prompt() {
        assert!(parse_fastboot_devices("< waiting for any device >").is_empty());
        assert!(parse_fastboot_devices("").is_empty());
    }

    #[test]
    fn getvar_keeps_only_known_keys() {
        let out = "(bootloader) product: o1s\n\
                   (bootloader) secure: yes\n\
                   (bootloader) mystery: 42\n\
                   OKAY [ 0.010s]\n";
        let vars = parse_getvar_all(out);
        assert_eq!(vars.get("product").map(String::as_str), Some("o1s"));
        assert_eq!(vars.get("secure").map(String::as_str), Some("yes"));
        assert!(!vars.contains_key("mystery"));
        assert!(!vars.contains_key("okay [ 0.010s]"));
    }

    #[test]
    fn getvar_handles_missing_prefix() {
        let vars = parse_getvar_all("current-slot: b\n");
        assert_eq!(vars.get("current-slot").map(String::as_str), Some("b"));
    }
}
