package com.cvby.cin

import android.content.Context
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material.icons.rounded.Memory
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.RestartAlt
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.cvby.cin.ui.CinTheme
import com.cvby.cin.ui.LoaderScreen
import com.cvby.cin.ui.ConfirmDialog
import com.cvby.cin.ui.MessageDialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        val home = CinCore.homeDir(this)
        val loadError = CinCore.ensureLoaded()

        // 工具链是 APK 自带的（adb + fastboot + 依赖库），
        // 首次启动时从 assets 展开到私有目录；之后靠 stamp 文件短路。
        val toolchain = if (loadError == null) Toolchain.ensure(this) else null

        setContent {
            CinTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background,
                ) {
                    HomeScreen(
                        context = this@MainActivity,
                        home = home,
                        loadError = loadError,
                        toolchain = toolchain,
                    )
                }
            }
        }
    }
}

/** 待确认的操作：持有用户将要执行的那份 Plan。 */
private data class Pending(val plan: CinCore.Plan)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HomeScreen(
    context: Context,
    home: String,
    loadError: String?,
    toolchain: Toolchain.Result?,
) {
    val scope = rememberCoroutineScope()

    // 工具链未就绪时，所有命令都不要发出去：
    // 否则用户只会看到 "Cannot run program adb" 这种没有信息量的报错。
    val toolchainError: String? = when (toolchain) {
        null -> null
        is Toolchain.Result.Failed -> toolchain.message
        is Toolchain.Result.Ready -> null
    }

    var devices by remember { mutableStateOf<List<CinCore.Device>>(emptyList()) }
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf(if (loadError != null) "核心未加载" else "点右上角刷新以扫描设备") }
    var pending by remember { mutableStateOf<Pending?>(null) }
    var dialog by remember { mutableStateOf<Pair<String, String>?>(null) }
    // true 时展示 EDL Loader 页面（导入/校验/列表），而不是设备列表
    var showLoaders by remember { mutableStateOf(false) }

    // ---- root 提权开关 ----
    // 默认关闭：行为与普通 App 完全一致。
    // 打开后 adb/fastboot 由 su 代执行，能越过 /dev/bus/usb、netlink、smartsocket 三道墙。
    // 选择持久化到 SharedPreferences，避免每次重启 App 都要重新点一遍。
    val prefs = remember { context.getSharedPreferences("cin", Context.MODE_PRIVATE) }
    var useRoot by remember { mutableStateOf(prefs.getBoolean("use_root", false)) }
    // root 可用性只在需要时探测（探测本身会触发 su 授权弹窗，不能无条件做）
    var rootStatus by remember { mutableStateOf<RootShell.Status?>(null) }

    fun setUseRoot(enabled: Boolean) {
        useRoot = enabled
        prefs.edit().putBoolean("use_root", enabled).apply()
        if (!enabled) {
            rootStatus = null
            RootShell.invalidate()
        }
    }

    fun refresh() {
        if (loadError != null) {
            dialog = "无法使用" to loadError
            return
        }
        if (toolchainError != null) {
            dialog = "工具链不可用" to toolchainError
            return
        }
        busy = true
        status = "正在扫描设备…"
        scope.launch {
            val res = withContext(Dispatchers.IO) { CinCore.devices(home) }
            busy = false
            when (res) {
                is CinCore.Result.Ok -> {
                    devices = res.value
                    // 区分「真的没设备」和「有设备但未授权」。
                    // 前者要用户去接数据线，后者要用户在对方设备上点「允许 USB 调试」，
                    // 混为一谈会造成毫无意义的排查（真机上踩过）。
                    val unauthed = res.value.count { it.mode == "unauthorized" }
                    status = when {
                        res.value.isEmpty() -> "未发现设备"
                        unauthed > 0 ->
                            "发现 ${res.value.size} 台设备，其中 $unauthed 台未授权：" +
                                "请在设备上点「允许 USB 调试」"
                        else -> "发现 ${res.value.size} 台设备"
                    }
                }
                is CinCore.Result.Err -> {
                    devices = emptyList()
                    status = "扫描失败"
                    dialog = "扫描失败" to res.message
                }
            }
        }
    }

    fun requestReboot(device: CinCore.Device, target: String) {
        val protocol = if (device.isFastboot) "fastboot" else "adb"
        // 关键：这里只是「问 Rust 要一份计划」，没有任何副作用
        when (val res = CinCore.rebootPlan(home, device.serial, protocol, target)) {
            is CinCore.Result.Ok -> pending = Pending(res.value)
            is CinCore.Result.Err -> dialog = "无法生成计划" to res.message
        }
    }

    fun execute(p: CinCore.Plan) {
        pending = null
        busy = true
        status = "正在执行…"
        scope.launch {
            val ready = toolchain as? Toolchain.Result.Ready
            val outcomes = withContext(Dispatchers.IO) {
                if (ready == null) {
                    emptyList()
                } else {
                    Runner.runAll(
                        commands = p.commands,
                        binDir = ready.binDir,
                        libDir = ready.libDir,
                        homeDir = java.io.File(home),
                        tmpDir = java.io.File(context.cacheDir, "cin"),
                        useRoot = useRoot,
                    )
                }
            }
            busy = false
            val failed = outcomes.firstOrNull { !it.ok }
            if (failed == null) {
                status = "完成"
                dialog = "执行完成" to outcomes.joinToString("\n\n") { o ->
                    "$ ${Runner.preview(o.argv)}\n" +
                        (o.stdout.ifBlank { "（无输出）" } + if (o.stderr.isNotBlank()) "\n[stderr] ${o.stderr}" else "")
                }
            } else {
                status = "执行失败"
                dialog = "执行失败（退出码 ${failed.exitCode}）" to
                    "$ ${Runner.preview(failed.argv)}\n\n${failed.stderr.ifBlank { failed.stdout }}"
            }
            // 无论成败都重新扫描：设备状态很可能已经变了
            refresh()
        }
    }
    // 整页切换：Loader 页面接管全部内容，不叠加在设备列表上
    if (showLoaders) {
        LoaderScreen(context = context, onBack = { showLoaders = false })
        return
    }

    // 版本只在首次组合时读一次；remember 保证不会因重组反复调用 native
    val version: String? = remember(loadError) {
        if (loadError != null) {
            null
        } else {
            when (val r = CinCore.version()) {
                is CinCore.Result.Ok -> r.value
                is CinCore.Result.Err -> null
            }
        }
    }


    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("CIN", fontWeight = FontWeight.Bold)
                        // 加载失败时不要显示成 "v?"：那会把“native 层没起来”
                        // 伪装成“版本号读不到”，真机排查时极易被误导（排查过 3 轮）。
                        // 这里直接显示原因缩写，并把完整错误放到下方状态栏里。
                        Text(
                            text = if (version != null) "v$version" else "native 未加载",
                            style = MaterialTheme.typography.labelSmall,
                            color = if (version != null) {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            } else {
                                MaterialTheme.colorScheme.error
                            },
                        )
                    }
                },
                actions = {
                    IconButton(onClick = { showLoaders = true }) {
                        Icon(Icons.Rounded.Memory, contentDescription = "EDL Loader")
                    }
                    IconButton(onClick = { refresh() }, enabled = !busy) {
                        Icon(Icons.Rounded.Refresh, contentDescription = "刷新")
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 16.dp),
        ) {
            StatusRow(busy = busy, status = status)

            RootSwitchBar(
                enabled = useRoot,
                status = rootStatus,
                busy = busy,
                onShowKey = {
                    dialog = "adb 密钥" to AdbIdentity.read(home).render()
                },
                onToggle = { want ->
                    if (!want) {
                        setUseRoot(false)
                        status = "已关闭 root，使用普通权限"
                    } else {
                        // 开启时立即探测一次，让用户当场知道到底能不能用，
                        // 而不是等到点刷新才报错。
                        status = "正在请求 root 授权…"
                        scope.launch {
                            val st = withContext(Dispatchers.IO) { RootShell.detect(refresh = true) }
                            rootStatus = st
                            if (st.available) {
                                setUseRoot(true)
                                status = "已启用 root，点右上角刷新"
                            } else {
                                setUseRoot(false)
                                dialog = "root 不可用" to st.detail
                                status = "root 不可用"
                            }
                        }
                    }
                },
            )

            if (devices.isEmpty() && !busy) {
                EmptyHint(useRoot = useRoot)
            } else {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    items(devices, key = { it.serial + it.mode }) { d ->
                        DeviceCard(
                            device = d,
                            enabled = !busy,
                            onReboot = { target -> requestReboot(d, target) },
                        )
                    }
                    item { Spacer(Modifier.height(16.dp)) }
                }
            }
        }
    }

    pending?.let { p ->
        ConfirmDialog(
            plan = p.plan,
            onConfirm = { execute(p.plan) },
            onDismiss = {
                pending = null
                status = "已取消"
            },
        )
    }

    dialog?.let { (title, msg) ->
        MessageDialog(title = title, message = msg) { dialog = null }
    }
}

@Composable
private fun StatusRow(busy: Boolean, status: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (busy) {
            CircularProgressIndicator(
                modifier = Modifier.size(14.dp),
                strokeWidth = 2.dp,
            )
            Spacer(Modifier.width(8.dp))
        }
        Text(
            text = status,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/**
 * root 提权开关。
 *
 * 放在设备列表上方而不是藏在设置里：真机实测下，不开启它 App 永远看不到设备，
 * 这是用户第一步就该做的操作，藏起来只会让人以为 App 坏了。
 */
@Composable
private fun RootSwitchBar(
    enabled: Boolean,
    status: RootShell.Status?,
    busy: Boolean,
    onToggle: (Boolean) -> Unit,
    onShowKey: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 4.dp),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "root 执行",
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 14.sp,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    text = when {
                        !enabled -> "关闭：以普通 App 权限运行，通常看不到 USB 设备"
                        status?.available == true ->
                            "已启用：${status.impl.label}" +
                                if (status.supportsMountMaster) " · mount-master" else ""
                        else -> "已启用（尚未验证）"
                    },
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(
                checked = enabled,
                onCheckedChange = onToggle,
                enabled = !busy,
            )
        }
        // 密钥入口：授权一直卡在 unauthorized 时，用户需要知道自己该授权哪把钥匙
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 14.dp, end = 14.dp, bottom = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "adb 密钥",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = onShowKey) {
                Text("查看指纹", fontSize = 12.sp)
            }
        }
    }
}

@Composable
private fun EmptyHint(useRoot: Boolean) {
    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = "没有检测到设备",
                style = MaterialTheme.typography.titleMedium,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                text = if (useRoot) {
                    "请用数据线连接设备，确认 USB 调试已开启，然后点右上角刷新。\n\n" +
                        "adb 与 fastboot 已随应用内置，无需另行准备。"
                } else {
                    "请用数据线连接设备，确认 USB 调试已开启，然后点右上角刷新。\n\n" +
                        "adb 与 fastboot 已随应用内置，无需另行准备。\n\n" +
                        "如果已连接并开了调试仍看不到设备：Android 10 以后普通 App " +
                        "无权读取 /dev/bus/usb，请打开上方的「root 执行」开关。"
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
    }
}

@Composable
private fun DeviceCard(
    device: CinCore.Device,
    enabled: Boolean,
    onReboot: (String) -> Unit,
) {
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
                    text = device.serial,
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 14.sp,
                    modifier = Modifier.weight(1f),
                )
                ModeBadge(device)
            }

            if (device.transportId != null) {
                Spacer(Modifier.height(4.dp))
                Text(
                    text = "transport_id: ${device.transportId}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            Spacer(Modifier.height(10.dp))

            if (!device.isAdbUsable && !device.isFastboot) {
                Text(
                    text = "此状态下无法操作，请检查设备授权或重新插拔",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            } else {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    FilledTonalButton(
                        onClick = { onReboot("system") },
                        enabled = enabled,
                        modifier = Modifier.weight(1f),
                    ) {
                        Icon(
                            Icons.Rounded.RestartAlt,
                            contentDescription = null,
                            modifier = Modifier.size(16.dp),
                        )
                        Spacer(Modifier.width(6.dp))
                        Text("重启", fontSize = 13.sp)
                    }
                    FilledTonalButton(
                        onClick = { onReboot("bootloader") },
                        enabled = enabled,
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("到 BL", fontSize = 13.sp)
                    }
                    FilledTonalButton(
                        onClick = { onReboot("recovery") },
                        enabled = enabled,
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("到 Rec", fontSize = 13.sp)
                    }
                }
            }
        }
    }
}

@Composable
private fun ModeBadge(device: CinCore.Device) {
    val color = when {
        device.isFastboot -> MaterialTheme.colorScheme.tertiary
        device.isAdbUsable -> MaterialTheme.colorScheme.primary
        else -> MaterialTheme.colorScheme.error
    }
    Text(
        text = device.displayMode,
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        color = color,
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .padding(horizontal = 8.dp, vertical = 2.dp),
    )
}