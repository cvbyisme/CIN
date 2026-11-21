//! JNI 导出层。
//!
//! 与 Kotlin 的契约：
//! - 所有函数都以 `jstring` 进出，内容一律是 JSON（UTF-8）；
//! - 返回的 JSON 顶层永远是 `{"ok": bool, "data": ..., "error": "..."}`，
//!   这样 Kotlin 侧只需要写一次解析，不用为每个函数单独处理异常；
//! - **Kotlin 不得自行拼命令**：它拿到的 `commands` 数组就是将要执行的 argv，
//!   拿到的 `render` 就是弹窗正文，两者出自同一份 `Plan`，不会出现
//!   "弹窗显示 A、实际执行 B" 的偏差。
//!
//! 这里的代码只负责数据搬运，真正的逻辑全在 core 的纯函数里。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;

use crate::confirm::Plan;
use crate::context::Context;
use crate::device::Device;
use crate::flash::{FlashPlan, FlashTask};
use crate::reboot::{self, Protocol, Target};

/// 把 Rust 的 `Result` 统一包成 `{"ok":..,"data":..,"error":..}`。
fn envelope<T: ToJson>(result: Result<T, String>) -> String {
    match result {
        Ok(v) => format!("{{\"ok\":true,\"data\":{},\"error\":null}}", v.to_json()),
        Err(e) => format!(
            "{{\"ok\":false,\"data\":null,\"error\":{}}}",
            json_string(&e)
        ),
    }
}

/// 极简 JSON 字符串转义。只处理 JSON 规范要求的字符，其余原样输出。
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_string_array(items: &[String]) -> String {
    let body = items
        .iter()
        .map(|s| json_string(s))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", body)
}

/// 可被序列化成 JSON 的数据。
trait ToJson {
    fn to_json(&self) -> String;
}

impl ToJson for () {
    fn to_json(&self) -> String {
        "null".to_string()
    }
}

impl ToJson for String {
    fn to_json(&self) -> String {
        json_string(self)
    }
}

impl ToJson for Vec<Device> {
    fn to_json(&self) -> String {
        let items = self
            .iter()
            .map(|d| {
                format!(
                    "{{\"serial\":{},\"mode\":{},\"rawState\":{},\"transportId\":{}}}",
                    json_string(&d.serial),
                    json_string(mode_str(d)),
                    json_string(&d.raw_state),
                    match &d.transport_id {
                        Some(t) => json_string(t),
                        None => "null".to_string(),
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{}]", items)
    }
}

fn mode_str(d: &Device) -> &'static str {
    use crate::device::Mode;
    match d.mode {
        Mode::Adb => "adb",
        Mode::AdbUnauthorized => "unauthorized",
        Mode::AdbOffline => "offline",
        Mode::AdbSideload => "sideload",
        Mode::Fastboot => "fastboot",
        Mode::Fastbootd => "fastbootd",
        Mode::Edl => "edl",
        Mode::Unknown => "unknown",
    }
}

impl ToJson for Plan {
    fn to_json(&self) -> String {
        let cmds = self
            .commands
            .iter()
            .map(|argv| json_string_array(argv))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"summary\":{},\"risk\":{},\"render\":{},\"commands\":[{}],\"consequences\":{}}}",
            json_string(&self.summary),
            json_string(&self.risk.to_string()),
            json_string(&self.render()),
            cmds,
            json_string_array(&self.consequences)
        )
    }
}

/// 从 jstring 取出 Rust String；空指针或非法 UTF-8 都变成 Err。
fn take_string(env: &mut JNIEnv, s: &JString) -> Result<String, String> {
    env.get_string(s)
        .map(|js| js.into())
        .map_err(|e| format!("读取 Java 字符串失败: {e}"))
}

/// 把 Rust String 转成 Java 字符串。
fn to_jstring(env: &mut JNIEnv, s: String) -> jstring {
    match env.new_string(s) {
        Ok(js) => js.into_raw(),
        Err(_) => {
            // 连返回字符串都创建失败时不能返回 null 让 Kotlin NPE，
            // 退化成一段固定的错误 JSON。
            let fallback = "{\"ok\":false,\"data\":null,\"error\":\"JNI 字符串创建失败\"}";
            match env.new_string(fallback) {
                Ok(js) => js.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
    }
}

/// 统一的外壳：捕获 panic（避免 unwind 穿过 FFI 边界导致进程 abort）、
/// 把结果包成 JSON 再转成 jstring。
fn guarded<F>(env: &mut JNIEnv, f: F) -> jstring
where
    F: FnOnce(&mut JNIEnv) -> Result<String, String>,
{
    let out = match catch_unwind(AssertUnwindSafe(|| f(env))) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => envelope::<()>(Err(e)),
        Err(_) => envelope::<()>(Err("core 内部 panic，已拦截".to_string())),
    };
    to_jstring(env, out)
}

fn make_context(home: Option<String>, assume_yes: bool) -> Context {
    Context::new(home.map(PathBuf::from), assume_yes)
}

// ---------------------------------------------------------------------------
// 导出函数
// ---------------------------------------------------------------------------

/// `cinVersion()` → `{"ok":true,"data":{"version":"0.2.0","platform":"aarch64"}}`
#[no_mangle]
pub extern "system" fn Java_com_cvby_cin_CinCore_nativeVersion(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    guarded(&mut env, |_| {
        // 必须走 envelope，与 devices / plan 等接口保持同一形状。
        // Kotlin 侧统一按 {"ok":..,"data":{..}} 解析，若此处返回扁平对象，
        // data 字段缺失会导致前端永远拿不到版本号（真机上表现为标题栏显示 v?）。
        Ok(envelope(Ok(format!(
            "{{\"version\":{},\"platform\":{}}}",
            json_string(env!("CARGO_PKG_VERSION")),
            json_string(crate::toolchain::platform_dir())
        ))))
    })
}

/// `cinDevices(home)` → 设备列表
#[no_mangle]
pub extern "system" fn Java_com_cvby_cin_CinCore_nativeDevices(
    mut env: JNIEnv,
    _class: JClass,
    home: JString,
) -> jstring {
    guarded(&mut env, |env| {
        let home = take_string(env, &home)?;
        let home = if home.trim().is_empty() {
            None
        } else {
            Some(home)
        };
        let ctx = make_context(home, true);
        // 设备发现是只读操作，工具链不在也不应该崩，而是返回明确错误
        ctx.prepare()?;
        Ok(envelope(Ok(ctx.discover())))
    })
}

/// `cinRebootPlan(home, serial, protocol, target)` → `Plan` JSON（**不执行**）
///
/// 这是 App 流程的关键一步：Kotlin 拿到 `render` 显示在 Dialog 里，
/// 用户点"确认"后，Kotlin 再把同一份 `commands` 逐条交给 runner 执行。
#[no_mangle]
pub extern "system" fn Java_com_cvby_cin_CinCore_nativeRebootPlan(
    mut env: JNIEnv,
    _class: JClass,
    home: JString,
    serial: JString,
    protocol: JString,
    target: JString,
) -> jstring {
    guarded(&mut env, |env| {
        let _ = take_string(env, &home)?; // 目前 reboot 不需要 home，占位保持签名一致
        let serial = take_string(env, &serial)?;
        let protocol = take_string(env, &protocol)?;
        let target_s = take_string(env, &target)?;

        let proto = match protocol.to_ascii_lowercase().as_str() {
            "adb" => Protocol::Adb,
            "fastboot" => Protocol::Fastboot,
            other => return Err(format!("未知协议 {other:?}，只支持 adb / fastboot")),
        };
        let tgt = Target::parse(&target_s).ok_or_else(|| {
            let known = Target::all()
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("未知重启目标 {target_s:?}，可选: {known}")
        })?;
        if serial.trim().is_empty() {
            return Err("缺少设备序列号".to_string());
        }

        Ok(envelope(Ok(reboot::plan(tgt, proto, &serial))))
    })
}

/// `cinFlashPlan(home, serial, protocol, partitions[], images[], rebootAfter)` → `Plan` JSON（**不执行**）
#[no_mangle]
pub extern "system" fn Java_com_cvby_cin_CinCore_nativeFlashPlan(
    mut env: JNIEnv,
    _class: JClass,
    home: JString,
    serial: JString,
    protocol: JString,
    partitions: jni::objects::JObjectArray,
    images: jni::objects::JObjectArray,
    reboot_after: jni::sys::jboolean,
) -> jstring {
    guarded(&mut env, |env| {
        let _ = take_string(env, &home)?;
        let serial = take_string(env, &serial)?;
        let protocol = take_string(env, &protocol)?;

        let proto = match protocol.to_ascii_lowercase().as_str() {
            "adb" => Protocol::Adb,
            "fastboot" => Protocol::Fastboot,
            other => return Err(format!("未知协议 {other:?}，只支持 adb / fastboot")),
        };

        let parts = read_string_array(env, &partitions)?;
        let imgs = read_string_array(env, &images)?;
        if parts.len() != imgs.len() {
            return Err(format!(
                "分区数 ({}) 与镜像数 ({}) 不一致",
                parts.len(),
                imgs.len()
            ));
        }

        let tasks = parts
            .into_iter()
            .zip(imgs)
            .map(|(p, i)| FlashTask::new(p, i))
            .collect::<Vec<_>>();
        let mut fp = FlashPlan::new(tasks);
        if reboot_after != 0 {
            fp = fp.with_reboot();
        }
        Ok(envelope(fp.to_plan(proto, &serial)))
    })
}

fn read_string_array(
    env: &mut JNIEnv,
    arr: &jni::objects::JObjectArray,
) -> Result<Vec<String>, String> {
    if arr.is_null() {
        return Ok(Vec::new());
    }
    let len = env
        .get_array_length(arr)
        .map_err(|e| format!("读取数组长度失败: {e}"))?;
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let obj = env
            .get_object_array_element(arr, i)
            .map_err(|e| format!("读取数组元素 {i} 失败: {e}"))?;
        let js = JString::from(obj);
        out.push(take_string(env, &js)?);
    }
    Ok(out)
}

/// 仅供测试：确认 envelope 的形状稳定。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::confirm::Risk;

    #[test]
    fn json_string_escapes_quotes_and_newlines() {
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\nb"), "\"a\\nb\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn envelope_ok_and_err_shapes() {
        let ok = envelope(Ok("x".to_string()));
        assert_eq!(ok, "{\"ok\":true,\"data\":\"x\",\"error\":null}");
        let bad = envelope::<()>(Err("boom".into()));
        assert_eq!(bad, "{\"ok\":false,\"data\":null,\"error\":\"boom\"}");
    }

    /// version 的 payload 必须被 envelope 包住，否则 Kotlin 侧读 data 恒为 null，
    /// 真机上表现为标题栏显示 "v?"。这条测试锁死形状，防止回归。
    #[test]
    fn version_payload_is_wrapped_in_envelope() {
        let payload = format!(
            "{{\"version\":{},\"platform\":{}}}",
            json_string("0.3.0"),
            json_string("aarch64")
        );
        let wrapped = envelope(Ok(payload));
        assert!(
            wrapped.starts_with("{\"ok\":true,\"data\":"),
            "缺少 data 字段: {wrapped}"
        );
        assert!(
            wrapped.contains("\"version\":\"0.3.0\""),
            "版本号丢失: {wrapped}"
        );
        assert!(
            wrapped.contains("\"platform\":\"aarch64\""),
            "平台丢失: {wrapped}"
        );
    }

    #[test]
    fn plan_json_includes_render_and_commands() {
        let plan = Plan::new("重启", Risk::Low).command(vec![
            "adb".to_string(),
            "-s".to_string(),
            "S1".to_string(),
            "reboot".to_string(),
        ]);
        let j = plan.to_json();
        assert!(j.contains("\"render\":"));
        assert!(j.contains("[\"adb\",\"-s\",\"S1\",\"reboot\"]"));
        assert!(j.contains("\"risk\":\"可恢复\""));
    }

    #[test]
    fn device_json_is_valid_array() {
        use crate::device::{parse_adb_devices, parse_fastboot_devices};
        let mut all = parse_adb_devices("ABC\tdevice\n");
        all.extend(parse_fastboot_devices("XYZ\tfastboot\n"));
        let j = all.to_json();
        assert!(j.starts_with('['));
        assert!(j.ends_with(']'));
        assert!(j.contains("\"serial\":\"ABC\""));
        assert!(j.contains("\"serial\":\"XYZ\""));
        assert!(j.contains("\"mode\":\"fastboot\""));
    }
}
