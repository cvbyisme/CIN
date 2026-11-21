package com.cvby.cin

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * Rust core 的 JNI 绑定。
 *
 * 契约（与 core/src/jni.rs 一一对应）：
 * - 所有原生方法都返回一个 JSON 字符串，形状固定为
 *   `{"ok": boolean, "data": any, "error": string?}`；
 * - **不与 Rust 争抢"命令怎么拼"的职责**：这里只负责把 `commands` 数组
 *   原样交给 [Runner]，不需要知道 fastboot 的语法。
 */
object CinCore {

    /** 一次操作的描述，与 Rust 侧 `Plan` 结构一致。 */
    data class Plan(
        val summary: String,
        val risk: String,
        val render: String,
        val commands: List<List<String>>,
        val consequences: List<String>,
    )

    data class Device(
        val serial: String,
        val mode: String,
        val rawState: String,
        val transportId: String?,
    ) {
        /** 是否处于 fastboot 系（含 fastbootd）。 */
        val isFastboot: Boolean get() = mode == "fastboot" || mode == "fastbootd"
        /** 是否可用于 adb 操作。 */
        val isAdbUsable: Boolean get() = mode == "adb" || mode == "sideload"
        /** 在 UI 上展示的模式名。 */
        val displayMode: String
            get() = when (mode) {
                "adb" -> "adb"
                "unauthorized" -> "未授权"
                "offline" -> "离线"
                "sideload" -> "sideload"
                "fastboot" -> "fastboot"
                "fastbootd" -> "fastbootd"
                "edl" -> "EDL"
                else -> "未知($rawState)"
            }
    }

    /** 一个可展示的结果：要么是数据，要么是错误消息。 */
    sealed interface Result<out T> {
        data class Ok<T>(val value: T) : Result<T>
        data class Err(val message: String) : Result<Nothing>
    }

    private var loaded: Boolean? = null

    private external fun nativeVersion(): String
    private external fun nativeDevices(home: String): String
    private external fun nativeRebootPlan(
        home: String,
        serial: String,
        protocol: String,
        target: String,
    ): String
    private external fun nativeFlashPlan(
        home: String,
        serial: String,
        protocol: String,
        partitions: Array<String>,
        images: Array<String>,
        rebootAfter: Boolean,
    ): String

    /**
     * 加载 libcin.so。
     *
     * 返回 null 表示成功，否则返回人类可读的失败原因
     * （通常是"这个 ABI 的 .so 没被打进 APK"）。
     */
    fun ensureLoaded(): String? {
        loaded?.let { return if (it) null else "libcin.so 之前加载失败" }
        return try {
            System.loadLibrary("cin")
            loaded = true
            null
        } catch (t: Throwable) {
            loaded = false
            "无法加载 libcin.so（${t.message}）。请确认 APK 中包含当前 ABI 的原生库。"
        }
    }

    // ------------------------------------------------------------------
    // 对外 API：全部返回 Result，不抛异常
    // ------------------------------------------------------------------

    fun version(): Result<String> = safe {
        val obj = JSONObject(nativeVersion())
        val data = obj.optJSONObject("data")
            ?: return@safe Result.Err(obj.optString("error", "version 返回为空"))
        Result.Ok("${data.optString("version")} (${data.optString("platform")})")
    }

    fun devices(home: String): Result<List<Device>> = safe {
        val data = envelopeData(nativeDevices(home))
            ?: return@safe Result.Err(lastError ?: "设备列表为空")
        val arr = data as? JSONArray ?: return@safe Result.Err("设备列表格式异常")
        val out = ArrayList<Device>(arr.length())
        for (i in 0 until arr.length()) {
            val o = arr.optJSONObject(i) ?: continue
            out += Device(
                serial = o.optString("serial"),
                mode = o.optString("mode", "unknown"),
                rawState = o.optString("rawState", ""),
                transportId = if (o.isNull("transportId")) null else o.optString("transportId"),
            )
        }
        Result.Ok(out)
    }

    /** 构造重启计划但**不执行**；返回的 [Plan.render] 就是要弹窗展示的内容。 */
    fun rebootPlan(home: String, serial: String, protocol: String, target: String): Result<Plan> =
        safe { parsePlan(nativeRebootPlan(home, serial, protocol, target)) }

    /** 构造刷写计划但**不执行**。 */
    fun flashPlan(
        home: String,
        serial: String,
        protocol: String,
        partitions: List<String>,
        images: List<String>,
        rebootAfter: Boolean,
    ): Result<Plan> = safe {
        parsePlan(
            nativeFlashPlan(
                home,
                serial,
                protocol,
                partitions.toTypedArray(),
                images.toTypedArray(),
                rebootAfter,
            )
        )
    }

    // ------------------------------------------------------------------
    // 内部工具
    // ------------------------------------------------------------------

    /** 最近一次 envelope 的 error 文本，仅在 [envelopeData] 返回 null 时有意义。 */
    private var lastError: String? = null

    /** 把所有异常收敛成 [Result.Err]，保证 UI 层永远不会收到未捕获异常。 */
    private inline fun <T> safe(block: () -> Result<T>): Result<T> = try {
        block()
    } catch (t: Throwable) {
        Result.Err("调用核心失败：${t.message}")
    }

    /** 解开 `{"ok":..}` 外壳，返回 data；失败时返回 null 并把原因存入 [lastError]。 */
    private fun envelopeData(json: String): Any? {
        lastError = null
        val obj = JSONObject(json)
        if (!obj.optBoolean("ok", false)) {
            lastError = obj.optString("error", "未知错误")
            return null
        }
        return obj.opt("data")
    }

    private fun parsePlan(json: String): Result<Plan> {
        val data = envelopeData(json) ?: return Result.Err(lastError ?: "计划为空")
        val o = data as? JSONObject ?: return Result.Err("计划格式异常")

        val cmds = ArrayList<List<String>>()
        o.optJSONArray("commands")?.let { arr ->
            for (i in 0 until arr.length()) {
                val inner = arr.optJSONArray(i) ?: continue
                val argv = ArrayList<String>(inner.length())
                for (j in 0 until inner.length()) argv += inner.optString(j)
                cmds += argv
            }
        }

        val cons = ArrayList<String>()
        o.optJSONArray("consequences")?.let { arr ->
            for (i in 0 until arr.length()) cons += arr.optString(i)
        }

        return Result.Ok(
            Plan(
                summary = o.optString("summary"),
                risk = o.optString("risk"),
                render = o.optString("render"),
                commands = cmds,
                consequences = cons,
            )
        )
    }

    /**
     * 工具链根目录：应用私有目录下的 `toolchain/`。
     *
     * 用户需要把 `adb`、`fastboot` 放到 `<home>/toolchain/bin/` 下。
     * 这里刻意使用私有目录而非 /sdcard，避免权限与命名空间问题。
     */
    fun homeDir(context: Context): String {
        val dir = File(context.filesDir, "cin").apply { mkdirs() }
        return dir.absolutePath
    }
}