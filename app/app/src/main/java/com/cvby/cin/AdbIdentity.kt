package com.cvby.cin

import java.io.File
import java.security.MessageDigest

/**
 * adb 客户端身份（RSA 密钥）的只读视图。
 *
 * 为什么需要它：adb 在连接一台新设备时，会用自己的私钥做一次挑战签名，
 * 设备端拿公钥去比对 /data/misc/adb/adb_keys 白名单。
 * 不在白名单里，状态就永远停在 `unauthorized`，**而且设备端不会再弹授权框**
 * （系统认为没有新请求），用户会误以为“已经授权过了、但还是不行”。
 *
 * 真机踩过的坑：App 用的是自己的密钥，跟用户之前在电脑上授权的那把不是同一把。
 * 所以必须能让用户看到当前这把钥匙的指纹，才能对上系统里的白名单条目。
 */
object AdbIdentity {

    /** 密钥文件的存放位置，必须与 Runner 传给 adb 的 HOME 一致。 */
    private fun keyFile(homeAbs: String): File =
        File(File(homeAbs, ".android"), "adbkey.pub")

    data class Info(
        /** 公钥文件是否存在 */
        val exists: Boolean,
        /** 公钥所在绝对路径，便于用户去系统里核对 */
        val path: String,
        /** SHA-256 指纹（十六进制，冒号分隔），不存在时为 null */
        val fingerprint: String?,
        /** 公钥原文（截断显示用） */
        val publicKey: String?,
    ) {
        /** 给用户看的简短说明，直接放进弹窗正文。 */
        fun render(): String = buildString {
            if (!exists) {
                append("尚未生成密钥。\n\n")
                append("点一次「刷新」让 adb 启动后，密钥才会被创建。\n")
                append("路径：$path")
            } else {
                append("路径：$path\n\n")
                append("SHA-256 指纹：\n")
                append(fingerprint ?: "（计算失败）")
                append("\n\n这台 App 用的是独立密钥，与你在电脑上授权过的不是同一把。")
                append("若设备一直显示未授权，需要把下面这把公钥加入设备的 ")
                append("adb_keys 白名单：\n\n")
                append(publicKey?.take(120) ?: "")
                if ((publicKey?.length ?: 0) > 120) append("…")
            }
        }
    }

    /** 读取当前密钥信息。不会创建密钥——创建由 adb 自己完成。 */
    fun read(homeAbs: String): Info {
        val f = keyFile(homeAbs)
        if (!f.isFile) {
            return Info(false, f.absolutePath, null, null)
        }
        return try {
            val raw = f.readText().trim()
            // 公钥是 base64，指纹对 base64 文本做 SHA-256
            val md = MessageDigest.getInstance("SHA-256")
            val digest = md.digest(raw.toByteArray(Charsets.UTF_8))
            val fp = digest.joinToString(":") { b -> "%02X".format(b) }
            Info(true, f.absolutePath, fp, raw)
        } catch (t: Throwable) {
            Info(true, f.absolutePath, null, null)
        }
    }
}
