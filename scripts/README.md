# Build Scripts

该目录集中维护项目的构建入口脚本。命令行构建统一通过 `build.sh` 执行，用于避免 macOS、Android 和 Windows 的构建命令分散在不同目录或文档中。

## 入口脚本

```bash
./scripts/build.sh <command>
```

## GitHub 自动发版

仓库包含以下 GitHub Actions 工作流：

- `.github/workflows/release.yml`

触发规则与行为：

- 监听 `main` / `master` 分支的 `push`
- 也支持手动触发 `workflow_dispatch`
- 自动读取 `Cargo.toml` 中 `[package].version`
- `push` 场景下仅当版本号相对上一次提交发生变化时才进入发版
- 如果对应的 `v<version>` tag 已存在，则自动跳过重复发版
- 满足条件后会构建 macOS、Windows、Android 产物，并在 GitHub 中创建或更新对应的 tag / Release

补充说明：

- Android 产物使用仓库现有的 `release` 构建流程；如果仓库未配置签名，GitHub Release 中的 APK 可能为未签名的 release 包
- 如需恢复某个既有版本的 Release 资产，可手动触发该工作流重新上传产物

可用命令：

- `android-env`：打印 Android 构建环境探测结果
- `android-setup`：通过 `sdkmanager` 安装 Android SDK/NDK 依赖
- `android-wrapper`：在 `android/` 目录生成 Gradle Wrapper
- `android-debug`：构建 Android Debug APK
- `android-release`：构建 Android Release APK
- `android-install`：安装 Android Debug APK 到已连接设备
- `windows-env`：打印 Windows 构建环境探测结果
- `windows-setup`：通过 `rustup` 安装 Windows Rust target，并检查 GNU 交叉工具链可见性
- `windows-release`：构建 Windows Release 可执行文件并归档到 `dist/`
- `macos-release`：构建 macOS release 二进制、`.app`、`.zip` 和 `.tar.xz`

## Android 构建

### 前置条件

- JDK 17
- Android SDK 34
- Android Build-Tools 34.0.0
- Android Platform-Tools
- Android NDK
- Rust 工具链
- `cargo-ndk`

脚本在检测到 `rustup` 时会优先使用 `rustup` toolchain，默认使用 `stable`，以避免混用 Homebrew `cargo`/`rustc` 与 `rustup target` 导致 target 不可见的问题。

### 推荐环境变量

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/<your-ndk-version>"
export CLIPLINK_RUSTUP_TOOLCHAIN="stable"
```

补充：

- `build.sh` 会优先尝试 `JAVA_HOME`
- 然后读取 `android/gradle.properties` 中的 `org.gradle.java.home`
- 在 macOS 上还会继续尝试 `/usr/libexec/java_home -v 17` 以及常见的 Homebrew / 系统 JDK 17 安装位置

### 常用流程

1. 先检查环境：

```bash
./scripts/build.sh android-env
```

2. 安装 Android SDK/NDK 依赖：

```bash
./scripts/build.sh android-setup
```

3. 如仓库中尚未包含 `android/gradlew`，先生成 Wrapper：

```bash
./scripts/build.sh android-wrapper
```

4. 构建 Debug APK：

```bash
./scripts/build.sh android-debug
```

5. 安装到真机：

```bash
./scripts/build.sh android-install
```

### 产物路径

- Debug APK: `android/app/build/outputs/apk/debug/app-debug.apk`
- Release APK: `android/app/build/outputs/apk/release/` 下的实际产物，常见为 `app-release.apk` 或 `app-release-unsigned.apk`

### Android 适配范围

- 提供原生 Activity 壳层，而不是类似 macOS 的托盘小弹窗
- 支持历史列表查询、激活、清空
- 支持偏好设置读写
- 支持设备信任/移除信任
- 支持 Android 系统剪贴板监听和回写
- 文件历史项回写系统剪贴板时，使用 `FileProvider` 暴露 `content://` `Uri`

### Android 平台差异

- 没有全局快捷键弹窗
- 使用前台服务维持局域网同步与历史状态刷新，但 Android 10+ 仍受系统剪切板前台访问限制
- `content://` 文件会先复制到应用缓存目录，再交给 Rust 核心处理

## Android 联调

1. macOS 与 Android 连接同一局域网
2. 双端都开启设备发现和本机历史共享
3. 在任一端设备列表中完成互相信任
4. 在 macOS 复制文本或文件，检查 Android 历史列表是否收到
5. 在 Android 点击历史项，检查系统剪贴板是否更新，并能在目标应用中粘贴
6. 再反向验证 Android 到 macOS 的同步与粘贴

## macOS Release 构建

```bash
./scripts/build.sh macos-release
```

输出目录为 `dist/`，会生成：

- `.app` 包
- `.zip`
- `.tar.xz`

## Windows Release 构建

### 前置条件

- Rust 1.85+，能够支持 edition 2024
- 推荐使用 `rustup`
- Windows 主机构建时：
  - Visual Studio Build Tools 或完整 Visual Studio
  - 已安装 `x86_64-pc-windows-msvc` target
- 非 Windows 主机交叉构建时：
  - 推荐 `x86_64-pc-windows-gnu`
  - 需要 `mingw-w64` 这类 GNU Windows 交叉工具链
  - 如果 `mingw-w64` 已在 `PATH` 中，脚本会自动探测并回填 target 专属 `LINKER/AR/CC/CXX`
  - `x86_64-pc-windows-msvc` 仍可手动指定，但在非 Windows 主机上通常还需要额外的 linker / SDK 支持

### 推荐环境变量

```bash
export CLIPLINK_WINDOWS_TARGET="x86_64-pc-windows-gnu"
```

如果是交叉构建，但脚本未能自动探测 linker，仍可手动设置，例如：

```bash
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="<your-linker>"
```

### 常用流程

1. 先检查环境：

```bash
./scripts/build.sh windows-env
```

2. 安装 Rust Windows target：

```bash
./scripts/build.sh windows-setup
```

3. 构建 Release 可执行文件：

```bash
./scripts/build.sh windows-release
```

### 产物路径

- 可执行文件：`dist/cliplink-windows-<target>.exe`
- Zip 归档：`dist/cliplink-windows-<target>.zip`
- Tar.xz 归档：`dist/cliplink-windows-<target>.tar.xz`

说明：

- `windows-release` 在 Windows 主机上默认使用 `x86_64-pc-windows-msvc`
- 在非 Windows 主机上，如果 `mingw-w64` 已可用，脚本会自动把默认目标切到 `x86_64-pc-windows-gnu`
- 如果脚本检测到了 `rustup`，会优先使用 `rustup run <toolchain> cargo/rustc`，默认 toolchain 为 `stable`
- 如果本机缺少 `zip`、`tar` 或 PowerShell 压缩能力，脚本仍会先保留 `.exe`，仅跳过对应归档格式

## 故障排查

- `android-env` 显示 `ANDROID_NDK_HOME=<not found>`：
  说明 NDK 尚未安装，先执行 `android-setup`
- 报错缺少 `gradle` 或 `android/gradlew`：
  先执行 `android-wrapper`，或者手动安装 Gradle
- 报错 JDK 版本不足：
  需要 JDK 17，低版本 JDK 无法用于现用 Android Gradle Plugin；可通过 `JAVA_HOME` 或 `android/gradle.properties` 的 `org.gradle.java.home` 显式指定
- 报错 Rust edition 2024 或 Android target 问题：
  更新 `rustup stable`，并安装 `aarch64-linux-android` target
- `windows-env` 显示 `WINDOWS_TARGET=<not configured>`：
  说明运行环境不是 Windows 主机，且脚本也没有自动探测到可用的 GNU Windows 交叉工具链
- `windows-release` 报错缺少 Windows target：
  先执行 `windows-setup`，或手动运行 `rustup target add <target>`
- `windows-release` 报错 linker / C toolchain 问题：
  Windows 构建涉及原生 Win32 和 C 依赖；优先安装 `mingw-w64` 并使用 `x86_64-pc-windows-gnu`，如果仍需特殊 toolchain，再手动设置 target 专属 `LINKER/AR/CC/CXX`
- 已通过 `rustup target add` 安装 target，但脚本仍提示 target 不存在：
  脚本会优先使用 `rustup`；如需手动执行构建命令，需确认实际使用的不是 Homebrew 提供的 `cargo/rustc`
