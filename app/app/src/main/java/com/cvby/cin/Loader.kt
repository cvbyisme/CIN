package com.cvby.cin

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.security.MessageDigest

/**
 * 用户自导入的 EDL programmer（firehose loader）。
 *
 * 本模块**只做导入、校验、列出**，不提供任何刷写能力。
 * 刷写链路仍未实现，UI 上也必须对用户明说，避免“导入了但点了没反应”。
 *
 * 设计理由：
 * 1. programmer 属于设备厂商资产，分发权不在本项目，因此绝不经网络下载；
 *    只能由用户从本地文件选择器指定；
 * 2. 文件始终保存在应用私有目录 <files>/cin/loaders/，不进 assets、不进仓库；
 * 3. 导入时只读首个 64KB 做 ELF 头部校验，不加载、不执行。
 */
object Loader {

    /** 导入结果。 */
    sealed interface Result {
        data class Ok(val info: Info) : Result
        data class Failed(val message: String) : Result
    }

    /** 已导入的 loader 元数据。 */
    data class Info(
        /** 展示用文件名（保留用户原文件名）。 */
        val name: String,
        /** 实际落盘路径。 */
        val path: String,
        val size: Long,
        /** 小写十六进制 SHA-256。 */
        val sha256: String,
        /** ELF 机器类型，如 "aarch64"；无法识别时为 null。 */
        val arch: String?,
    ) {
        val sizeText: String
            get() = when {
                size >= 1024 * 1024 -> "%.1f MB".format(size / 1024.0 / 1024.0)
                size >= 1024 -> "%.1f KB".format(size / 1024.0)
                else -> "$size B"
            }
    }

    private fun dir(context: Context): File =
        File(CinCore.homeDir(context), "loaders").apply { mkdirs() }

    /** 已导入全部 loader，按文件名排序。 */
    fun list(context: Context): List<Info> {
        val d = dir(context)
        val files = d.listFiles() ?: return emptyList()
        return files
            .filter { it.isFile && !it.name.startsWith(".") }
            .sortedBy { it.name.lowercase() }
            .map { f ->
                Info(
                    name = f.name,
                    path = f.absolutePath,
                    size = f.length(),
                    sha256 = runCatching { sha256Of(f) }.getOrDefault(""),
                    arch = runCatching { readElfArch(f) }.getOrNull(),
                )
            }
    }

    /** 删除一个已导入的 loader。只允许删除 dir 下的文件，防止路径穿越。 */
    fun remove(context: Context, path: String): Boolean {
        val f = File(path).canonicalFile
        val d = dir(context).canonicalFile
        if (f.parentFile?.canonicalPath != d.canonicalPath) return false
        return f.delete()
    }

    /**
     * 从 SAF Uri 导入一个文件。
     *
     * 校验收得很松，因为各家厂商的 programmer 并非都严格是 ELF：
     * - 必须是普通文件、体积在 (1KB, 256MB] 区间；
     * - 若头部是 ELF，则要求 e_type 为 ET_EXEC/ET_DYN（不强制）；
     * 不是 ELF 也允许导入，但会在 arch 里标为 null，由 UI 提醒用户自行确认。
     */
    fun import(context: Context, uri: Uri): Result {
        val cr = context.contentResolver
        val display = queryName(context, uri) ?: "loader-${System.currentTimeMillis()}.elf"
        // 只保留文件名，去掉可能的路径片段
        val safeName = display.substringAfterLast('/').substringAfterLast('\\')
            .replace(Regex("[^A-Za-z0-9._-]"), "_")
            .take(96)
            .ifBlank { "loader.elf" }

        return try {
            val target = File(dir(context), safeName)
            cr.openInputStream(uri).use { input ->
                if (input == null) return Result.Failed("无法读取所选文件")
                target.outputStream().use { out -> input.copyTo(out) }
            }
            val size = target.length()
            if (size < 1024L) {
                target.delete()
                return Result.Failed("文件太小（$size 字节），不像是 programmer")
            }
            if (size > 256L * 1024 * 1024) {
                target.delete()
                return Result.Failed("文件太大（${size / 1024 / 1024} MB），已拒绝导入")
            }
            Result.Ok(
                Info(
                    name = target.name,
                    path = target.absolutePath,
                    size = size,
                    sha256 = sha256Of(target),
                    arch = runCatching { readElfArch(target) }.getOrNull(),
                )
            )
        } catch (t: Throwable) {
            Result.Failed("导入失败：${t.message ?: t.javaClass.simpleName}")
        }
    }

    private fun queryName(context: Context, uri: Uri): String? = runCatching {
        context.contentResolver.query(uri, null, null, null, null)?.use { c ->
            val idx = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (idx >= 0 && c.moveToFirst()) c.getString(idx) else null
        }
    }.getOrNull()

    private fun sha256Of(f: File): String {
        val md = MessageDigest.getInstance("SHA-256")
        f.inputStream().use { input ->
            val buf = ByteArray(64 * 1024)
            while (true) {
                val n = input.read(buf)
                if (n <= 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().joinToString("") { "%02x".format(it) }
    }

    /**
     * 读 ELF 头 e_machine，返回可读的架构名。
     * 只读前 20 字节，不 mmap、不执行。
     */
    private fun readElfArch(f: File): String? {
        val head = ByteArray(20)
        val n = f.inputStream().use { it.read(head) }
        if (n < 20) return null
        if (head[0] != 0x7f.toByte() || head[1] != 'E'.code.toByte() ||
            head[2] != 'L'.code.toByte() || head[3] != 'F'.code.toByte()
        ) return null
        val little = head[5] == 1.toByte()
        val machine = if (little) {
            (head[18].toInt() and 0xff) or ((head[19].toInt() and 0xff) shl 8)
        } else {
            ((head[18].toInt() and 0xff) shl 8) or (head[19].toInt() and 0xff)
        }
        return when (machine) {
            0xB7 -> "aarch64"
            0x28 -> "arm"
            0x3E -> "x86_64"
            0x03 -> "x86"
            else -> "unknown(0x%x)".format(machine)
        }
    }
}
