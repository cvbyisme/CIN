plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "com.cvby.cin"
    compileSdk = 35

    // 正式签名（cvby 密钥，SHA-256 61:9D:B3:F6:...:DF:2D）。
    // 为什么需要：debug 签名每次构建都不同，导致新包无法覆盖旧包，
    // 每次升级都得先卸载、丢失全部设置与已生成的 adbkey。
    // 凭据从环境变量 / gradle.properties 读，不写进仓库：
    //   CIN_KEYSTORE           keystore 路径
    //   CIN_KEYSTORE_PASSWORD  store password
    //   CIN_KEY_ALIAS          密钥别名（默认 cvby）
    //   CIN_KEY_PASSWORD       key password（缺省同 store password）
    // 四者缺一则不启用正式签名，CI 会退回 debug 签名且不会报错。
    val ksPath = System.getenv("CIN_KEYSTORE")
        ?: (project.findProperty("CIN_KEYSTORE") as String?)
    val ksPass = System.getenv("CIN_KEYSTORE_PASSWORD")
        ?: (project.findProperty("CIN_KEYSTORE_PASSWORD") as String?)
    val ksAlias = System.getenv("CIN_KEY_ALIAS")
        ?: (project.findProperty("CIN_KEY_ALIAS") as String?)
        ?: "cvby"
    val ksKeyPass = System.getenv("CIN_KEY_PASSWORD")
        ?: (project.findProperty("CIN_KEY_PASSWORD") as String?)
        ?: ksPass

    signingConfigs {
        if (ksPath != null && ksPass != null) {
            create("cvby") {
                storeFile = file(ksPath)
                storePassword = ksPass
                keyAlias = ksAlias
                keyPassword = ksKeyPass
            }
        }
    }

    defaultConfig {
        applicationId = "com.cvby.cin"
        minSdk = 29
        targetSdk = 35
        versionCode = 30
        versionName = "0.3.0"

        ndk {
            // 与 core 的交叉编译目标保持一致；x86_64 用于模拟器调试
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
            // 有 cvby 密钥时用它签 debug 包，使产物可覆盖安装。
            // 没有则沿用默认 debug 签名，保证本地开发依旧能直接构建。
            signingConfigs.findByName("cvby")?.let { signingConfig = it }
        }
        release {
            isMinifyEnabled = false
            signingConfigs.findByName("cvby")?.let { signingConfig = it }
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    // .so 由 CI 在 assemble 之前注入到 jniLibs，本地没有也不报错
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    // 工具链随 APK 分发（adb / fastboot + 依赖库），不能压缩：
    // 1) aapt 默认只对 *.so 免压缩，无扩展名的 adb/fastboot 会被 deflate，
    //    解压出来再 chmod 虽然也能跑，但首次启动更慢、且体积优势本来就不大；
    // 2) 免压缩后 assets 在 APK 内按 4 字节对齐存储，读取更快。
    androidResources {
        // assets 里的工具链不能被 aapt 压缩：
        // aapt 默认只对 *.so 免压缩，而 adb/fastboot 没有扩展名，会被 deflate。
        // 两害都显式列入，避免首次启动解压耗时。
        noCompress += listOf("adb", "fastboot", "so")
        // 保留 aapt 默认忽略规则，但去掉 “!.*” 式排除。
        // toolchain 下只有 bin/lib 和 manifest.txt，无需额外过滤。
        ignoreAssetsPattern = "!.svn:!.git:!.ds_store:!*.scc:!CVS:!thumbs.db:!picasa.ini:!*~"
    }

    // .so 必须解压到 /data/app/.../lib/<abi>/ 后再加载。
    //
    // 这里用 useLegacyPackaging = true（= extractNativeLibs=true），
    // 代价是 APK 里多一份解压后的副本，换来的是不依赖 zip 对齐：
    //
    // 背景：useLegacyPackaging=false 时系统直接从 APK 里 mmap .so，
    // 要求 so 的 zip data offset 必须 4096 对齐，否则 dlopen 直接失败，
    // 抛 UnsatisfiedLinkError。实测 CI 产物对齐结果：
    //   arm64-v8a/libcin.so   data_offset%4096 = 1957
    //   armeabi-v7a/libcin.so data_offset%4096 = 3211
    //   x86_64/libcin.so      data_offset%4096 = 2618
    // 三者全部未对齐，真机上表现为标题栏 “v?” + 永远“未发现设备”，
    // 因为 loadError 非空，整个 native 层（含设备枚举）从未启动。
    // AGP 的 noCompress + STORED 并不保证 4096 对齐，故不用该模式。
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.activity.compose)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.extended)
}