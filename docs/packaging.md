# 桌面客户端打包

## GitHub Actions 自动构建

`.github/workflows/ci.yml` 目前构建 macOS 和 Windows。每次 push、Pull Request，或在 Actions 的 **Build → Run workflow** 手动运行时，分别在原生宿主上执行 `cargo fmt`、Clippy、`cargo test --locked --workspace` 和 Release 打包：

| Runner | 工具链目标 | 产物 |
| --- | --- | --- |
| `macos-15` | `aarch64-apple-darwin` | `MijiaLAN-Rust-<version>-macos-arm64.zip`、`.sha256` |
| `macos-15-intel` | `x86_64-apple-darwin` | `MijiaLAN-Rust-<version>-macos-x86_64.zip`、`.sha256` |
| `windows-2022` | `x86_64-pc-windows-msvc` | `MijiaLAN-Rust-<version>-windows-x86_64.zip`、`.sha256` |

Rust 固定为本机已验证的 1.98.1，使用 `Cargo.lock`，缓存按编译目标隔离。macOS 使用 Xcode 26.3，并在需要时安装 Metal Toolchain；Windows 初始化 Visual Studio x64 编译环境，Release 脚本查找 SDK 的 `fxc.exe` 并嵌入预编译着色器。为控制 GPUI 在标准 runner 上的内存和磁盘占用，CI 使用 2 个编译任务，关闭增量构建和 Debug / Test 调试符号。

macOS 复用 `scripts/build-macos.sh release`，包含桌面 App、CLI、图标和许可文件；打包前核对架构与签名。Windows 复用 `scripts/build-windows.ps1 -Profile release` 生成便携 ZIP。两边都会解压 ZIP，并运行其中的 CLI 帮助与内存演示设备查询。此检查不访问真实设备或凭据，也不代替桌面界面或真实 HomeKit 配对验收。

在对应运行的 **Artifacts** 下载产物，保留 14 天。同一分支的新运行会取消旧运行。工作流只需要仓库读取权限，无需配置证书或 Secrets；macOS 使用临时签名且未公证，Windows 未签名。CI 不生成 MSI / NSIS，不自动创建 GitHub Release；正式分发签名、公证和发布需另行配置。Linux 依赖与本地构建仍由 `scripts/docker/Dockerfile`、`scripts/build-linux.sh` 提供。

## macOS Release

当前正式体验包面向 Apple Silicon。`scripts/package-macos-release.sh` 从锁定依赖构建优化的 Release 程序，先运行工作区 Release 测试，再组装 `MijiaLAN.app`；内置 CLI、应用图标、隐私说明、项目和第三方许可证。Rust 数据标识和目录保持不变，与 Swift 版独立。

```sh
MIJIALAN_SIGNING_IDENTITY='Developer ID Application: Your Name (TEAMID)' \
MIJIALAN_NOTARY_PROFILE='your-existing-notary-profile' \
MIJIALAN_BUILD_NUMBER=3 \
bash scripts/package-macos-release.sh
```

脚本对 CLI 和 App 使用 Developer ID、Hardened Runtime 和安全时间戳，分别公证 App 与 DMG、附加并验证公证票据，再检查 Gatekeeper。输出位于 `build/releases/v<version>-rs.<build>/dist/`，包含 `MijiaLAN-rs-<version>-b<build>-macos-arm64.dmg`、ZIP 和 `SHA256SUMS.txt`；不同 build 不覆盖已有体验包。DMG 支持拖到 Applications；ZIP 中也保留已附加票据的 App。

日志保存在相邻的 `logs/`。若已签名的 staging App 不变，可以用同一环境变量加 `--package-existing` 重试公证与封装；不会重新编译。打包脚本不安装或替换当前客户端，也不会上传 GitHub Release。已有本地 Rust 用户首次改用 Developer ID 版本时，系统可能要求重新允许该签名读取凭据。

`build-macos.sh` 的默认本地 Debug 包仍叫 `MijiaLAN Rust.app`，便于并行开发；可通过 `MIJIALAN_APP_OUTPUT`、`MIJIALAN_APP_NAME` 和 `MIJIALAN_BUILD_NUMBER` 指定单独 staging 路径、显示名和 build 号。`resources/macos/AppIcon.icns` 从现有应用 SVG 生成，保留 Lucide 许可。

## 当前候选包

2026-11-04 使用 Rust 1.98、GPUI 0.2.2、Debian 12 x64 构建容器生成：

- `MijiaLAN-Rust-0.1.0-windows-x86_64-debug.msi`
- `MijiaLAN-Rust-0.1.0-windows-x86_64-debug.zip`
- `mijialan-rust-0.1.0-linux-x86_64-debug.deb`
- `mijialan-rust-0.1.0-linux-x86_64-debug.tar.gz`

目录为 `artifacts/`，各平台附 `.sha256` 校验清单。这一批是 Debug 测试包，关闭了调试符号；尚无 Windows 代码签名。Windows MSI 已在群晖虚拟机完成安装和同版本升级，当前紧凑托盘版通过 Microsoft Remote Desktop 更新并实际显示，Escape 收起后进程仍存活；验收边界见 `verification.md`。Linux 桌面交互尚未验收。包内只有程序、说明和许可证，不包含 Mac 的设备 Token、主密钥、设备清单或家庭桥身份。

## Windows 原生构建

需要 Rust stable、Visual Studio C++ Build Tools、Windows SDK、CMake。Release 还需要 SDK 的 `fxc.exe`，脚本会查找它并设置 `GPUI_FXC_PATH`。使用 Visual Studio Developer PowerShell：

```powershell
./scripts/build-windows.ps1 -Profile release
# 可选：-Profile debug 或 -Target aarch64-pc-windows-msvc
```

原生脚本默认生成 ZIP。x64 ZIP 可交给 Linux 构建容器中的 `scripts/package-windows-msi.sh` 生成 MSI；打包工具是 GNOME msitools / wixl。MSI 按当前用户安装至 `%LOCALAPPDATA%\Programs\MijiaLAN Rust`，创建正常与演示模式的开始菜单入口，使用 HKCU 组件记录，没有自定义执行动作。它不会配置开机启动或修改防火墙；卸载目标只有安装文件与入口，用户数据和凭据不在安装组件中。MSVC / ARM64 路径尚未在本轮实际构建。

最初的 NSIS 安装器被此虚拟机的 Defender 标记为 `Trojan:Win32/Bearfoos.A!ml`，未执行，记录显示已处置；这不能直接认定为误报。初版便携包及嵌入清单 / 着色器修复版均另行完成哈希核对、解包与 Defender 自定义扫描，没有相关新增检测，实时保护保持开启。默认分发入口已切换为标准 MSI；NSIS 仅保留为 `MIJIALAN_BUILD_NSIS=1` 或 PowerShell `-Nsis` 的实验选项，不能将该旧 EXE 视为通过验收的安装包。

本次 Windows 包使用 GNU 工具链交叉构建。GPUI 的 Windows Release 构建脚本只在 Windows 主机执行着色器编译，因此 Linux 容器目前仅生成 Windows Debug 包；它在 Windows 运行时使用系统 `d3dcompiler_47.dll` 编译着色器。需要较新的 Windows 10 / Windows 11，并有可用的 Direct3D 设备。不能据此承诺群晖虚拟显卡兼容。

跨主机构建会漏掉 GPUI 只在 Windows 主机嵌入的清单，已由应用构建脚本补齐 Common Controls v6、DPI 元数据与普通用户执行级别，解决首次 `0xC0000139` 加载失败。GPUI Debug 的 HLSL 已改为嵌入并从内存编译，避免依赖构建机的 Cargo 路径；详见 `vendor/gpui/PATCHES.md`。Windows GUI 入口使用 Windows 子系统（PE Subsystem 2），Debug 也不再附加控制台；CLI 使用控制台子系统（3）。

Windows 现与 Mac 共用 360 DIP 宽的紧凑面板，使用无标题栏的 `PopUp` 窗口。原生代码按托盘所在显示器的 DPI 和工作区定位，限制高度以适应小屏；Windows 区域裁剪提供虚拟机也能显示的圆角。GPUI 的弹出窗口明确使用 `WS_POPUP`，不再套用普通窗口的隐形边框偏移；最后一个窗口关闭后也不再无条件结束消息循环，退出由应用的托盘状态和显式退出动作决定。应用的房屋图标以多尺寸 ICO 嵌入 EXE，并用于托盘；按钮 SVG 也在 Debug 包中嵌入，分发时不依赖源码目录。区域选择使用原生 Windows 弹出菜单。Linux 保留桌面窗口。

原生窗口定位和改高安排到当前 GPUI 更新结束之后执行，避免同步 `WM_SIZE` 回调无法借用窗口而留下旧视口。演示入口会在标题和底栏明确标出模拟设备；它只改变内存数据，不会控制真实硬件。请使用普通入口，并在这台 Windows 中单独扫码导入设备。

Windows 原生下拉菜单同样直接交给前台执行器，跟踪期间不持有 GPUI 的 App 或 Window 借用。`App::defer` 在刷新副作用时仍持有 App 借用，不能用于包裹 `TrackPopupMenu`：原生菜单会泵送消息并运行输入框光标等异步任务，从而触发重复借用崩溃。菜单命令编号由 `menu_selection::selected_index` 解码，使用 `checked_sub(1)` 并校验范围；取消返回 0 时不能用会提前计算减法的 `then_some`。

## Linux 原生构建

开发依赖见 `scripts/docker/Dockerfile`。

```sh
bash scripts/build-linux.sh release
# 测试包：bash scripts/build-linux.sh debug
```

脚本在 x64 生成 amd64 DEB / x86_64 tar.gz，在 ARM64 生成 arm64 包。本轮只验证 x64 构建。当前容器基于 Debian 12，包要求 glibc 2.36 或更新；适用于满足依赖的 Debian 12、Ubuntu 24.04 等桌面系统。

```sh
sudo apt install ./mijialan-rust-0.1.0-linux-x86_64-debug.deb
mijialan-rust --demo
```

桌面需要 X11 或 Wayland、Vulkan 驱动和字体；GNOME 的托盘可能需要 StatusNotifierItem 扩展。真实模式还需要用户会话中可用、已解锁的 Secret Service。无桌面的 NAS / 容器环境不能只复制二进制后就假设凭据服务可用；当前程序不会降级为明文保存。

GPUI 的传递依赖 `xattr 0.2.3` 在新版 libc 上引用了被移除的 Linux 常量，已通过 `vendor/xattr` 的局部补丁修复，说明与双许可证均保留。

## 在 Mac 使用构建容器

先启动 Docker Desktop，然后在项目根目录：

```sh
docker build --platform linux/amd64 -t mijialan-rust-builder:1.98 -f scripts/docker/Dockerfile .
mkdir -p artifacts

docker run --rm --platform linux/amd64 \
  -e CARGO_TARGET_DIR=/target -e MIJIALAN_ARTIFACT_DIR=/out \
  -v mijialan-rust-cargo-registry:/usr/local/cargo/registry \
  -v mijialan-rust-cargo-git:/usr/local/cargo/git \
  -v mijialan-rust-linux-target:/target \
  -v "$PWD:/workspace:ro" -v "$PWD/artifacts:/out" \
  mijialan-rust-builder:1.98 bash scripts/build-linux.sh debug

docker run --rm --platform linux/amd64 \
  -e CARGO_TARGET_DIR=/target -e MIJIALAN_ARTIFACT_DIR=/out \
  -v mijialan-rust-cargo-registry:/usr/local/cargo/registry \
  -v mijialan-rust-cargo-git:/usr/local/cargo/git \
  -v mijialan-rust-windows-target:/target \
  -v "$PWD:/workspace:ro" -v "$PWD/artifacts:/out" \
  mijialan-rust-builder:1.98 bash -c \
  'cargo build --locked --target x86_64-pc-windows-gnu -p mijialan-desktop -p mijialan-cli && bash scripts/package-windows.sh debug x86_64-pc-windows-gnu'
```

源代码只读挂载，构建缓存使用独立卷。Mac 的正常 `.app`、凭据和桥接身份不进入容器。两个目标使用不同的 target 卷，避免彼此覆盖。

## 群晖 Windows 虚拟机验收

先使用演示模式检查窗口、系统托盘、中文字体、设置、调光和关闭重开，再检查正常模式的凭据写入、扫码导入、LAN 读写和家庭桥。Windows 版使用独立主密钥和家庭桥身份，不自动读取 Mac 数据。

群晖 VMM 的虚拟显卡能否提供 GPUI 所需 Direct3D 设备，需要实际启动确认；安装包生成或后端协议测试不能代替这一步。网络还需让虚拟机与设备在可互通的 LAN，允许相应 UDP 广播、mDNS 和桥接 TCP。不要通过关闭系统防火墙来完成验收。

先在群晖 `win11` 控制台验证 Windows `10.0.26200.7840`、800×600、Microsoft 基本显示适配器下的中文界面、设置与安装版 CLI，Windows 33 项后端测试通过。随后按用户要求改用 Microsoft Remote Desktop 更新紧凑托盘版：MSI 已扫描、升级返回 0，安装载荷哈希一致，紧凑卡片与圆角实际可见，Escape 收起后进程仍存活。Windows 后续已扫码导入 11 台真实设备，重启后可读取凭据并连接 2 台在线设备，台灯 CLI 写入及 GUI 调光均经独立 LAN 读回验证。最终菜单修复版实际检查持续展开、取消、选择、收起及托盘重开后，同一进程仍运行且标准错误为空。Windows 的本轮 iPhone 配对尚未验收。详细证据见 [跨平台验收记录](platform-smoke-2026-11-04.md)。

## Apple TV

GPUI 0.2.2 只有桌面平台后端，当前项目没有 tvOS 构建目标、电视界面或签名入口。Rust 本身提供 [tvOS 编译目标](https://doc.rust-lang.org/rustc/platform-support/apple-tvos.html)，但这不能为 GPUI 补上电视窗口和遥控器交互；需要独立 tvOS 客户端及凭据等平台适配。

用户明确的电视需求是让 Apple TV 常驻运行家庭桥。普通 tvOS 应用退到后台后通常会挂起；公开的后台模式用于音频、系统调度任务等特定用途，没有适合这类第三方局域网 TCP / mDNS 桥的通用常驻模式。因而不能承诺在看视频、返回主屏或休眠时持续提供桥接，即便单独做出 tvOS 前台客户端也不能解决这个生命周期问题。依据见 [Apple 后台执行说明](https://developer.apple.com/documentation/xcode/configuring-background-execution-modes)。

更适合的架构是让 Mac 或 NAS 运行桥接进程，Apple TV 作为系统家庭中枢承担远程访问和自动化，参见 [Apple 家庭中枢说明](https://support.apple.com/en-us/102557)。当前 Linux 桌面包依赖 Secret Service，不能直接将其视为已经完成验收的群晖无界面服务；NAS 常驻部署还需要专门的凭据保存、持久目录、生命周期与网络验证。本轮未创建 tvOS 空壳客户端，也未改变 NAS 部署或 Apple TV 系统配置。
