package com.cvby.cin

import java.io.File

/**
 * root 提权通道，支持 Magisk / KernelSU / APatch（含 FolkPatch 等衍生版）。
 *
 * 为什么需要它（真机实测，机型 2405CPX3DC / Android 16 / APatch）：
 *
 * 工具链里的 adb 以普通 App 身份启动时，会同时撞上三道墙：
 *   1. usb_linux.cpp:633  Failed to open bus directory '/dev/bus/usb': Permission denied
 *      —— /dev/bus/usb/NNN/MMM 是 crw-rw---- root usb，App 的 gid 列表里没有 1014(usb)
 *   2. usb_linux.cpp:668  Failed to open netlink socket: Permission denied
 *      —— SELinux 禁止 untrusted_app 创建 NETLINK_KOBJECT_UEVENT 套接字，收不到热插拔事件
 *   3. main.cpp:167       could not install *smartsocket* listener: Operation not permitted
 *      —— Android 10+ 禁止普通 App 监听/连接抽象 unix socket 做代理
 *
 * 这三道墙是系统的权限模型，代码绕不过去。所以「在手机上跑 adb 控制另一台设备」
 * 必须以 root 执行。
 *
 * ── 关于三种 root 方案的行为差异（实测得出，不能想当然）──
 *
 * 1) 「环境变量」不能作为 su 的裸参数。
 *    实测：`su PATH=/xxx adb devices` → `su: unknown user: PATH=/xxx`
 *    因为 su 的用法是 `su [options] [-] [user [argument...]]`，第一个位置参数被当成用户名。
 *    正确做法：`su -c "env K=V ... cmd args"`，用 env(1) 作中间层。
 *
 * 2) 「mount namespace」是必须处理的，否则 root 子进程看不到 App 私有目录。
 *    实测对比（同一路径 /data/data/com.cvby.cin/files/cin/）：
 *      su -c   "ls ..."  → No such file or directory
 *      su -M -c "ls ..." → loaders  tmp  toolchain   ✅
 *    原因：APatch/KernelSU 用 overlayfs 实现挂载，默认进入的是隔离的 mount ns。
 *    Magisk 没有 -M 参数，但默认就在 global mount namespace，不传也对。
 *    所以：支持 -M 就传，不支持就不传（探测式，而不是硬编码）。
 *
 * 3) 「单参数字符串」限制。
 *    实测：`su -c sh -c '...'` 这种嵌套会把整串当成单个文件名而报
 *    "inaccessible or not found"。必须只传一个完整的命令字符串给 -c。
 *    但这与「不拼 shell 字符串」的安全性目标冲突，因此本项目采用
 *    「严格转义的单字符串」而不是裸拼接，见 [quote]。
 */
object RootShell {

    /** 探到的 root 后端种类，用于在界面上如实标注。 */
    enum class Impl(val label: String) {
        APATCH("APatch"),
        KERNEL_SU("KernelSU"),
        MAGISK("Magisk"),
        UNKNOWN("未知 su"),
    }

    /**
     * 探测结果。
     *
     * @param available   是否真的拿到了 uid=0
     * @param suPath      可用的 su 路径
     * @param impl        后端种类
     * @param supportsMountMaster su 是否支持 -M/--mount-master
     * @param detail      人类可读的说明/失败原因，直接显示在界面上
     */
    data class Status(
        val available: Boolean,
        val suPath: String?,
        val impl: Impl,
        val supportsMountMaster: Boolean,
        val detail: String,
    )

    /** su 的候选路径，按优先级。 */
    private val SU_CANDIDATES = listOf(
        "/system/bin/su",
        "/system/xbin/su",
        "/sbin/su",
        "/su/bin/su",
        "/debug_ramdisk/su",
        "/data/adb/ksu/bin/su",
    )

    @Volatile
    private var cached: Status? = null

    /**
     * 探测 root 是否可用，并顺带确定后端与能力位。
     *
     * 探测本身会跑几次 su，可能触发授权弹窗，因此结果要缓存；
     * 用户刚在系统里授过权时调 [invalidate] 再试。
     */
    fun detect(refresh: Boolean = false): Status {
        cached?.let { if (!refresh) return it }

        val su = SU_CANDIDATES.firstOrNull { File(it).exists() }
            ?: return Status(false, null, Impl.UNKNOWN, false, "未找到 su，设备可能没有 root")
                .also { cached = it }

        // 第一步：确认真的能拿到 uid=0（su 存在但未授权时必须如实报不可用）
        val basic = exec(listOf(su, "-c", "id -u"), timeoutMs = 12_000)
        if (basic == null || basic.trim() != "0") {
            val why = if (basic == null) "su 无响应（可能在等待授权弹窗）"
            else "su 已拒绝授权：${basic.trim().take(160)}"
            return Status(false, su, Impl.UNKNOWN, false, why).also { cached = it }
        }

        // 第二步：认出后端。三者都提供 su -v，输出里带名字。
        val ver = (exec(listOf(su, "-v")) ?: "") + " " + (exec(listOf(su, "-V")) ?: "")
        val impl = when {
            ver.contains("APatch", ignoreCase = true) -> Impl.APATCH
            ver.contains("KernelSU", ignoreCase = true) ||
                ver.contains("ksu", ignoreCase = true) -> Impl.KERNEL_SU
            ver.contains("Magisk", ignoreCase = true) -> Impl.MAGISK
            else -> Impl.UNKNOWN
        }

        // 第三步：探测 -M 是否被接受。
        // 不能只看版本号写死：FolkPatch 等衍生版行为可能不同，实探最可靠。
        val mm = exec(listOf(su, "-M", "-c", "id -u"))?.trim() == "0"

        val detail = buildString {
            append(impl.label)
            if (mm) append("（支持 mount-master）")
            append(" · uid=0  已验证")
        }
        return Status(true, su, impl, mm, detail).also { cached = it }
    }

    fun invalidate() {
        cached = null
    }

    /**
     * 构造「以 root 执行 argv」的完整命令行。
     *
     * 因为 APatch/KSU 的 su 不接受任意 argv，只能走 `-c <单个命令字符串>`，
     * 所以这里必须把 argv 序列化成一个 shell 命令。为了不引入注入风险，
     * 每个参数都用 [quote] 做严格单引号转义，环境变量同样处理。
     *
     * @param st      [detect] 的结果
     * @param argv    原始 argv，argv[0] 为裸名时会被解析成工具链绝对路径
     * @param env     要注入的环境变量
     * @param binDir  工具链 bin 目录
     */
    fun wrap(st: Status, argv: List<String>, env: Map<String, String>, binDir: File): List<String> {
        require(argv.isNotEmpty()) { "argv 不能为空" }
        val suPath = st.suPath ?: error("root 不可用")

        // core 给的是裸名 adb/fastboot；root 的 PATH 与我们不同，换绝对路径才稳。
        val resolved = if (argv[0].contains('/')) argv
        else listOf(File(binDir, argv[0]).absolutePath) + argv.drop(1)

        // env(1) 作为中间层：su 不继承父进程环境，必须显式赋值。
        // toybox 的 env 在 /system/bin/env，是 Android 自带，三种 root 方案下都在。
        val envAssign = env.map { (k, v) -> "$k=${quote(v)}" }
        val cmd = (listOf("/system/bin/env") + envAssign + resolved.map { quote(it) })
            .joinToString(" ")

        val out = ArrayList<String>()
        out += suPath
        // APatch/KSU 必须进 global mount namespace 才能看到 /data/data/<pkg>；
        // Magisk 无此参数但在 global ns，故按实探结果决定。
        if (st.supportsMountMaster) out += "-M"
        out += "-c"
        out += cmd
        return out
    }

    /**
     * POSIX shell 单引号转义：把 ' 替换成 '\'' 并用单引号包起来。
     * 这样任何字符（空格、$、`、"、反斜杠、换行）都不会被解释。
     */
    private fun quote(s: String): String = "'" + s.replace("'", "'\\''") + "'"

    /** 跑一条命令并返回 stdout（合并 stderr），失败/超时返回 null。 */
    private fun exec(argv: List<String>, timeoutMs: Long = 8_000): String? = try {
        val p = ProcessBuilder(argv).apply { redirectErrorStream(true) }.start()
        val text = p.inputStream.bufferedReader().use { it.readText() }
        if (!p.waitFor(timeoutMs, java.util.concurrent.TimeUnit.MILLISECONDS)) {
            p.destroyForcibly()
            null
        } else {
            text
        }
    } catch (t: Throwable) {
        null
    }
}
