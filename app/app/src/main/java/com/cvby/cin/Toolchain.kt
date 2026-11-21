package com.cvby.cin

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import java.io.File
import java.io.FileOutputStream

/**
 * 把 APK assets 里的工具链展开到应用私有目录。
 *
 * 为什么必须解压：APK 里的 assets 不是真实文件系统路径，
 * 无法作为 execve 的目标，也不能被 linker 用作库搜索目录。
 * 所以首次启动时把 <assets>/toolchain/<abi>/ 整个复制到
 * <files>/cin/toolchain/，跑 chmod 755 后才能真正执行。
 *
 * 幂等性靠一个 stamp 文件：内容是本 APK 的 versionCode + 设备 ABI。
 * 只要两者没变，就不重复解压；升级 APK 或换 ABId 时自动重解。
 */
object Toolchain {

    /** 解压结果。 */
    sealed interface Result {
        data class Ready(val binDir: File, val libDir: File) : Result
        data class Failed(val message: String) : Result
    }

    private const val ASSET_ROOT = "toolchain"

    /** 当前设备的主 ABI，必须与 CI 产出的目录名一致。 */
    fun primaryAbi(): String = Build.SUPPORTED_ABIS.firstOrNull() ?: "arm64-v8a"

    private fun versionCode(context: Context): Long = try {
        val pm = context.packageManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            pm.getPackageInfo(context.packageName, PackageManager.PackageInfoFlags.of(0)).longVersionCode
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageInfo(context.packageName, 0).longVersionCode
        }
    } catch (t: Throwable) {
        -1L
    }

    /**
     * 确保工具链已就绪。可在主线程调用（首次解压约几百毫秒）。
     */
    fun ensure(context: Context): Result {
        val abi = primaryAbi()
        val root = File(CinCore.homeDir(context), "toolchain")
        val binDir = File(root, "bin")
        val libDir = File(root, "lib")

        val stamp = File(root, ".stamp")
        val want = "${versionCode(context)}:$abi"

        if (stamp.isFile && stamp.readText().trim() == want &&
            binDir.isDirectory && libDir.isDirectory
        ) {
            return Result.Ready(binDir, libDir)
        }

        return try {
            root.deleteRecursively()
            binDir.mkdirs()
            libDir.mkdirs()

            val assets = context.assets
            val base = "$ASSET_ROOT/$abi"

            // assets 不支持列目录的递归 API，必须显式列出文件名。
            // 清单由 CI 写入 assets/toolchain/<abi>/manifest.txt，
            // 每行形如 "bin/adb" 或 "lib/libz.so.1"。
            val manifest = assets.open("$base/manifest.txt")
                .bufferedReader()
                .use { it.readLines() }
                .filter { it.isNotBlank() }

            var count = 0
            for (entry in manifest) {
                val out = File(root, entry)
                out.parentFile?.mkdirs()
                assets.open("$base/$entry").use { input ->
                    FileOutputStream(out).use { output -> input.copyTo(output) }
                }
                // adb / fastboot 必须可执行；.so 不需要但设上无害
                out.setExecutable(true, false)
                out.setReadable(true, true)
                count++
            }

            // 自检：两个二进制真的在，否则后面的所有调用都会失败得莫名其妙
            for (name in listOf("adb", "fastboot")) {
                val f = File(binDir, name)
                if (!f.isFile || !f.canExecute()) {
                    return Result.Failed("工具链不完整：${f.absolutePath} 缺失或不可执行")
                }
            }

            stamp.writeText(want)
            Result.Ready(binDir, libDir)
        } catch (t: Throwable) {
            Result.Failed("展开工具链失败：${t.message}")
        }
    }
}
