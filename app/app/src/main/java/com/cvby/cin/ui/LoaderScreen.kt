package com.cvby.cin.ui

import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.DeleteOutline
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.cvby.cin.Loader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * 「EDL Loader」页面。
 *
 * 边界声明必须写在界面里，不能只写在文档里：
 * 用户在这里看到的必须包括「导入有什么后果」和「现在还不能做什么」。
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LoaderScreen(
    context: Context,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var items by remember { mutableStateOf<List<Loader.Info>>(emptyList()) }
    var message by remember { mutableStateOf<Pair<String, String>?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun reload() {
        scope.launch {
            items = withContext(Dispatchers.IO) { Loader.list(context) }
        }
    }

    // 首次进入就扫一次
    remember(Unit) { reload(); Unit }

    val picker = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        busy = true
        scope.launch {
            val res = withContext(Dispatchers.IO) { Loader.import(context, uri) }
            busy = false
            when (res) {
                is Loader.Result.Ok -> {
                    val i = res.info
                    message = "导入成功" to buildString {
                        append("文件：${i.name}\n")
                        append("大小：${i.sizeText}\n")
                        append("架构：${i.arch ?: "未识别（可能不是 ELF）"}\n")
                        append("SHA-256：${i.sha256}\n\n")
                        append("已保存到应用私有目录。请核对上面的指纹与你获取文件时的预期是否一致。")
                    }
                    reload()
                }
                is Loader.Result.Failed -> message = "导入失败" to res.message
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("EDL Loader", fontWeight = FontWeight.Bold) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Text("返回", fontSize = 13.sp)
                    }
                },
                actions = {
                    IconButton(
                        onClick = { picker.launch(arrayOf("*/*")) },
                        enabled = !busy,
                    ) {
                        Icon(Icons.Rounded.Add, contentDescription = "导入")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
            )
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item { BoundaryNotice() }

            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = if (items.isEmpty()) "尚未导入任何 loader" else "已导入 ${items.size} 个",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(
                        onClick = { picker.launch(arrayOf("*/*")) },
                        enabled = !busy,
                    ) {
                        Text("选择文件")
                    }
                }
            }

            items(items, key = { it.path }) { info ->
                LoaderCard(
                    info = info,
                    onDelete = {
                        scope.launch {
                            val ok = withContext(Dispatchers.IO) { Loader.remove(context, info.path) }
                            message = if (ok) {
                                "已删除" to info.name
                            } else {
                                "删除失败" to "无法删除 ${info.name}"
                            }
                            reload()
                        }
                    },
                )
            }

            item { Spacer(Modifier.height(16.dp)) }
        }
    }

    message?.let { (title, body) ->
        MessageDialog(title = title, message = body) { message = null }
    }
}

@Composable
private fun BoundaryNotice() {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant,
        ),
    ) {
        Column(modifier = Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    Icons.Rounded.WarningAmber,
                    contentDescription = null,
                    modifier = Modifier.size(18.dp),
                    tint = MaterialTheme.colorScheme.tertiary,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    text = "只导入，暂不刷写",
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
            }
            Spacer(Modifier.height(8.dp))
            Text(
                text = buildString {
                    append("programmer（firehose loader）是设备厂商的签名资产，")
                    append("本应用不会提供、也不会代为下载，只能由你从本地导入。\n\n")
                    append("当前版本只做导入、校验与列出，\n**EDL 刷写链路尚未实现**，")
                    append("导入后不会有任何刷写动作。\n\n")
                    append("去哪里找：向设备厂商的售后/官方固件包索取，")
                    append("或从你已购买的维修资料中提取。请勿使用来源不明的文件。")
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun LoaderCard(info: Loader.Info, onDelete: () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
    ) {
        Column(modifier = Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = info.name,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 13.sp,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onDelete) {
                    Icon(
                        Icons.Rounded.DeleteOutline,
                        contentDescription = "删除",
                        modifier = Modifier.size(18.dp),
                    )
                }
            }
            Text(
                text = "${info.sizeText}　架构：${info.arch ?: "未识别"}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (info.sha256.isNotBlank()) {
                Spacer(Modifier.height(6.dp))
                Text(
                    text = "SHA-256 ${info.sha256.take(16)}…${info.sha256.takeLast(8)}",
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
