package com.cvby.cin

import java.io.File

/**
 * 执行 core 给出的 argv。
 *
 * 这是整个 App 里**唯一**真正启动进程的地方。它接收的参数永远是
 * Rust 侧 `Plan.commands` 里的完整 argv（二进制名 + 全部参数），
 * 因此这里不需要、也不允许拼接任何 shell 字符串。
 *
 * 用 `ProcessBuilder(listOf(...))` 而不是 `sh -c "..."`，
 * 这样参数里的空格、引号、分号都不会被解释，从根上避免命令注入。
 */
object Runner {

    data class Outcome(
        val argv: List<String>,
        val exitCode: Int,
        val stdout: String,
        val stderr: String,
    ) {
        val ok: Boolean get() = exitCode == 0
    }

    /**
     * 依次执行 [commands]，遇到非零退出码立即停止。
     *
     * @param binDir 工具链 bin 目录，会被塞进子进程的 PATH 最前面
     */
    fun runAll(
        commands: List<List<String>>,
        binDir: File,
        libDir: File,
        homeDir: File,
        tmpDir: File,
        useRoot: Boolean = false,
        onProgress: (index: Int, total: Int, argv: List<String>) -> Unit = { _, _, _ -> },
    ): List<Outcome> {
        val results = ArrayList<Outcome>(commands.size)
        commands.forEachIndexed { i, argv ->
            onProgress(i, commands.size, argv)
            val outcome = runOne(argv, binDir, libDir, homeDir, tmpDir, useRoot)
            results += outcome
            if (!outcome.ok) return results
        }
        return results
    }

    /**
     * 执行单条命令。
     *
     * 工具链里的 adb / fastboot 是动态链接的，除了系统库（libc、liblog 等）
     * 还依赖打包进来的 libprotobuf、libc++_shared 等。这些库放在私有目录，
     * Android 10+ 不会自动搜索，必须显式通过 LD_LIBRARY_PATH 告诉 linker。
     */
    fun runOne(
        argv: List<String>,
        binDir: File,
        libDir: File,
        homeDir: File,
        tmpDir: File,
        useRoot: Boolean = false,
    ): Outcome {
        require(argv.isNotEmpty()) { "argv 不能为空" }
        // 先算出子进程需要的环境变量，普通执行与 root 执行共用同一套，
        // 避免两条路径行为不一致（真机上调过一轮才对齐）。
        val binDirAbs = binDir.absolutePath
        val libDirAbs = libDir.absolutePath
        val homeAbs = homeDir.absolutePath
        homeDir.mkdirs()
        tmpDir.mkdirs()

        val inheritedPath = System.getenv("PATH").orEmpty()
        val env = linkedMapOf(
            "PATH" to if (inheritedPath.isEmpty()) binDirAbs else "$binDirAbs:$inheritedPath",
            "LD_LIBRARY_PATH" to libDirAbs,
            // adb/fastboot 启动时会去 $HOME/.android 建目录；HOME 为空时会拼出
            // "//.android" 落到只读根目录上直接 abort（adb_utils.cpp:316）。真机实测。
            "HOME" to homeAbs,
            // 不设 ANDROID_USER_HOME！这也是真机实测出来的坑：
            // 一旦设了它，adb 会把 adbkey 写到别处，HOME/.android 下始终没有密钥，
            // 于是授权握手永远停在 unauthorized，且设备端也不会再弹授权框。
            // 实测对比（同一设备同一条命令）：
            //   HOME + ANDROID_USER_HOME -> .android 空目录，无 adbkey，unauthorized
            //   仅 HOME                  -> adbkey + adbkey.pub 正常生成
            "TMPDIR" to tmpDir.absolutePath,
        )

        // root 模式下由 su 代执行；su 不继承父进程环境，故 env 通过 argv 显式传递。
        val rootStatus = if (useRoot) RootShell.detect() else null
        if (useRoot && rootStatus?.available != true) {
            val why = rootStatus?.detail ?: "未探测 root"
            return Outcome(argv, -1, "", "需要 root 才能执行 ${argv.firstOrNull()}：$why")
        }

        val realArgv = if (useRoot) {
            // 注意：返回的是 `su [-M] -c "<转义后的完整命令串>"`，
            // 而不是原始 argv——因为 APatch/KSU 的 su 只吃 -c 的单字符串。
            RootShell.wrap(rootStatus!!, argv, env, binDir)
        } else {
            argv
        }

        return try {
            val pb = ProcessBuilder(realArgv).apply {
                // 两条路径（普通 / root）共用同一套环境变量
                environment().putAll(env)

                directory(File("/"))
                redirectErrorStream(false)
            }
            val proc = pb.start()
            // 必须先收完输出再 waitFor，否则输出量大时会因管道缓冲写满而互锁
            val out = proc.inputStream.bufferedReader().use { it.readText() }
            val err = proc.errorStream.bufferedReader().use { it.readText() }
            val code = proc.waitFor()
            Outcome(argv, code, out, err)
        } catch (t: Throwable) {
            Outcome(argv, -1, "", "无法执行 ${argv.firstOrNull()}：${t.message}")
        }
    }

    /** 把一条 argv 渲染成可直接阅读的形式（仅用于日志展示）。 */
    fun preview(argv: List<String>): String = argv.joinToString(" ")
}