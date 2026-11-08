# MijiaLAN · 飞牛 fnOS 原生版

本仓库为 MijiaLAN 的飞牛衍生版本。安装、功能与构建说明请先阅读 [README-fnOS.md](README-fnOS.md)。当前版本为 **0.1.1 测试版 / Intel、AMD x86_64**。

默认构建无桌面的 Rust 服务。下文保留上游桌面项目介绍，桌面命令不适用于本版本的默认 workspace。

---

# 上游 MijiaLAN

米家局域网客户端与 HomeKit 桥。此为 `MijiaLAN-rs` 分支，使用 Rust、GPUI 和 GPUI Component；仓库与产品名称仍为 MijiaLAN。Swift 版本保留在 `main` 分支，Rust 工程可直接从当前分支根目录构建。

macOS 使用顶部菜单栏，Windows 使用右下角系统托盘，两者共享紧凑设备面板：左键切换面板，点击面板外部或按 Escape 收起，右键打开设置或退出。面板收起后仍保持设备轮询与 HomeKit 服务。Windows 面板按托盘所在显示器的工作区和 DPI 定位，并使用独立的房屋应用图标。Linux 使用系统托盘和桌面窗口；托盘采用 StatusNotifierItem，无需 GTK。

Mac 界面沿用原 Swift 客户端的布局：360 pt 宽菜单栏面板，在线设备双列磁贴、离线设备三列网格，台灯亮度条、空调温控和分组设置。窗口按页面内容调整高度，长列表可滚动；外观可选择自动、浅色或深色。语言可在原生应用菜单「语言 / Language」中切换。控件由 GPUI 绘制，字体细节和转场与 SwiftUI 仍有差异。

Mac 的服务器选择使用 AppKit 原生 `NSMenu`，在 macOS 26 及更新版本随系统呈现 Liquid Glass，旧系统使用对应的原生菜单外观。支持选中勾选、键盘操作及 Escape 取消；关闭菜单会保留设置面板，再按 Escape 可收起面板。地区选择在本次运行中随面板重开保留。

## 运行

需要 Rust stable 与对应平台的编译工具。本机使用 Rust 1.98.1、macOS 和 Xcode 验证。

```sh
# 正常运行：首次扫码导入或手动添加设备
cargo run --bin mijialan-rust

# 演示：内存中的模拟设备，不读取真实凭据、不控制真实设备
cargo run --bin mijialan-rust -- --demo
```

macOS 打包：

```sh
./scripts/build-macos.sh debug
open "build/MijiaLAN Rust.app"
```

首次编译 GPUI 需要完整的 Xcode 与 Metal Toolchain。若提示缺少 Metal 编译工具，执行 `xcodebuild -downloadComponent MetalToolchain`。本地包使用临时签名，尚未进行 Developer ID 签名或 Apple 公证。

临时签名的代码身份会随重新构建变化，macOS 可能再次要求允许本应用访问自己的钥匙串凭据；这时无需重新扫码导入。需要反复更新本地构建时，可以指定本机已有的固定签名身份：`MIJIALAN_SIGNING_IDENTITY="Apple Development: …" ./scripts/build-macos.sh debug`。脚本会分别签名 CLI 和 App；这两个可执行文件首次读取凭据时可能分别需要系统授权。

Apple Silicon 的正式体验包使用 Release 优化、Developer ID 签名及 Hardened Runtime，App 和 DMG 分别公证并附加票据。生成 DMG、ZIP 和 SHA-256 清单：

```sh
MIJIALAN_SIGNING_IDENTITY='Developer ID Application: Your Name (TEAMID)' \
MIJIALAN_NOTARY_PROFILE='your-existing-notary-profile' \
bash scripts/package-macos-release.sh
```

安装包内 App 显示名为 MijiaLAN，保留 `xyz.ipocket.MijiaLAN.Rust` 标识及已有 Rust 数据目录。打包不会发布 GitHub Release；复现步骤和验收边界见 [macOS 打包说明](docs/packaging.md#macos-release)。

Windows 构建需要 Visual Studio C++ Build Tools、Windows SDK、CMake，以及用于收集分发许可的 Python 3.11+；CI 自动配置 Python 3.12。Linux 需要 C/C++ 编译工具、Clang、字体、X11 / Wayland 和 Vulkan 开发包，依赖清单见 [scripts/docker/Dockerfile](scripts/docker/Dockerfile)。GNOME 显示托盘图标通常还需要 StatusNotifierItem / AppIndicator 扩展。

Windows / Linux 打包入口：

```powershell
# 在 Windows 的 Visual Studio Developer PowerShell 中运行，生成便携 ZIP
./scripts/build-windows.ps1 -Profile release
```

```sh
# 在 Linux 开发环境中运行，生成 DEB 与 tar.gz
bash scripts/build-linux.sh release
```

Windows / Linux 产物位于 `artifacts/`，包含 SHA-256 清单。Linux x64 构建容器生成 Windows GNU Debug MSI / ZIP 和 Linux Debug DEB / tar.gz；这些是未签名的测试候选包。群晖 Windows 11 已验证紧凑托盘、扫码导入、凭据重读及台灯 GUI 调光和独立 LAN 读回；Windows 的 iPhone 家庭配对尚未验收。Linux 已完成干净 Debian 容器安装、CLI 运行及 33 项核心 / HomeKit 测试，Linux 桌面尚未验收。原生 Release 构建、容器复现与验证边界见 [打包说明](docs/packaging.md)。

GPUI 0.2.2 没有 tvOS 后端，不能将当前桌面项目直接打成 Apple TV 安装包。电视端需要单独的 tvOS 界面、遥控器焦点交互和平台适配；共享 Rust 协议核心可作为移植基础。

## 功能

- miIO UDP 54321 通信、设备时钟同步、请求 ID 匹配、AES-CBC 与 MD5 数据包校验。
- 广播发现与按设备 ID 自动更新 IP；在线轮询与离线指数退避。
- 手动添加设备：先通过握手和 `miIO.info` 验证，再保存凭据。
- 米家扫码导入，支持 cn、de、us、sg、ru、i2 地区；导入完成后丢弃登录会话。扫码任务独立于面板，收起后可继续等待手机确认。
- 内置 10 个型号的 MIoT 规格，其他型号可从 miot-spec.org 下载并缓存。
- 早期 miIO 型号沿用转换后的 hass-xiaomi-miot 属性表，并保留原项目手工核对的台灯、插座与空调伴侣映射。
- 扫地机对齐原生版：家庭主开关开启清扫、关闭回充，保留停止、扫拖模式和风速控制；按各型号规格解释状态并确认动作结果。旧石头机型使用明确的 `get_status` / `app_*` 映射，房间与分区等带厂商参数的动作不开放。支持范围及验收边界见 [扫地机说明](docs/vacuum-parity-2026-11-05.md)。
- 根据规格生成开关、选项、滑块和读数。写入前验证访问权限、数值范围和步长；无参数定义的属性只读。
- Mac 设备信息提供与原版一致的「兼容性说明」折叠列表；按设备规格列出尚未适配、参数不完整及可通过现有控件替代的能力，支持中英文。
- 厨房设备参数可先选择为草稿，再单独执行规格中已确认的烹饪动作；不会因为选择菜单而自动开始烹饪。
- HomeKit：SRP Pair Setup、Ed25519 / X25519 Pair Verify、ChaCha20-Poly1305 连接、mDNS 广播、控制器权限和状态通知。
- HomeKit 的数据库、读取与事件共用类型、范围和步长校验；异常读数标为不可用，写入请求在设备单位转换前验证。按实际支持模式提供必需特征，复杂规格超出桥接上限时保留主功能并在家庭桥信息中提示。
- HomeKit 原生服务覆盖灯、插座、风扇、净化器、加湿器、除湿机、取暖器、空调、窗帘及传感器；剩余能力使用具名开关和预设表达。
- 桥接身份、配对、AID / IID 和配置编号持久化；设备离线不移除配件；逐台选择是否桥接，支持 Configured Name。
- 桥接时可阻止闲置休眠，不阻止显示器关闭；可单独关闭此选项。
- 中英文界面、浅色 / 深色外观、房间筛选和键盘快捷键。关闭面板后释放视图，保留页面、滚动位置和参数草稿。
- Mac 滑块连续调节并合并快速拖动中的数值；手动数值输入保留独立的「应用」操作。设备菜单支持重连及选择是否显示在家庭中，重置家庭配对需要再次确认。

暂不支持蓝牙、Zigbee / Mesh 网关子设备、云灯组，以及没有已确认 LAN 命令映射的设备；与原项目的局域网边界一致。原 Swift 客户端的数据和 HomeKit 身份不会自动读入，新客户端需要重新导入和配对。

## 凭据与隐私

本项目使用独立的服务标识 `xyz.ipocket.MijiaLAN.Rust`。

- 普通设备清单不含 Token；设备 Token、HomeKit 私钥和配对信息写入带随机 nonce 的 ChaCha20-Poly1305 加密文件。
- 32 字节主密钥保存在 macOS Keychain、Windows Credential Manager 或 Linux Secret Service。这样不会受到 Windows 单个凭据项大小限制。
- 凭据存储不可用、密钥丢失或认证失败时报告错误，不回退到明文，不覆盖已有数据。
- 小米云登录会话只存在内存中；日常控制使用 LAN，不提供自建云、遥测或广告服务。
- `--demo` 完全使用内存凭据。示例 Token 均为测试数据。

Linux 真实使用需要可用且已解锁的 Secret Service。无桌面的服务环境也需要为运行用户配置相应凭据服务。

首次联网可能触发 macOS「本地网络」、LuLu 或 Windows 防火墙提示。设备发现需要 UDP 广播；HomeKit 需要 TCP 与 mDNS。请按实际用途允许此应用，不需要关闭系统防火墙。详情见 [PRIVACY.md](PRIVACY.md)。

## CLI / 后台运行

```sh
cargo run -p mijialan-cli -- discover
cargo run -p mijialan-cli -- list
cargo run -p mijialan-cli -- add 192.168.31.20 --name "书桌台灯"
cargo run -p mijialan-cli -- login --region cn
cargo run -p mijialan-cli -- refresh
cargo run -p mijialan-cli -- set <did> 2.1 true
cargo run -p mijialan-cli -- bridge
```

`add` 交互式读取 Token，不将其放在命令行参数中。`list` 不输出凭据。`bridge` 常驻运行，Ctrl+C 停止。GUI 与 CLI 使用同一个数据目录；避免同时启动两个桥接实例。

## 结构

| 路径 | 职责 |
| --- | --- |
| `crates/core` | 协议、发现、规格、旧型号适配、扫码导入、凭据与共享设备状态 |
| `crates/homekit` | 配对、加密 TCP、mDNS、权限、能力映射和事件通知 |
| `crates/desktop` | GPUI 界面、系统托盘、macOS 原生面板与视图状态 |
| `crates/cli` | 探测、导入、控制与无界面桥接 |
| `resources` | 内置规格和旧型号映射 |

GPUI 固定为 0.2.2，GPUI Component / Assets 固定为与其兼容的 0.5.1，避免框架 API 漂移。HAP 的 SRP 部分使用 hap-crypto 1.4.0，其余配对状态机、连接、桥接和设备映射在本项目中实现。

## 验证

### GitHub 自动构建

[Build 工作流](.github/workflows/ci.yml) 并行验证 Windows x64、Mac ARM64 和 Mac Intel，使用统一 Release profile 与原生 target，并缓存 GPUI 和依赖。每个平台的 Artifact 包含程序、许可、校验值及源码验证记录；Mac ARM 使用原生 Apple Silicon runner。

[Signed desktop release](.github/workflows/release-macos.yml) 复用同一提交的 Build 产物，完成 Mac 签名、公证和双平台封装，避免在 tag 上重复编译。手动运行默认仅生成预览；显式发布或 push rs tag 后自动发布 Rust prerelease，保留 Swift Latest。Windows 包未签名，需 Microsoft Visual C++ v14 x64 运行库。缓存命中、操作步骤和验证边界见 [CI 与发布说明](docs/ci-performance.md)。

### 本地检查

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

测试包含独立 python-miio 数据包向量、真实 UDP 回环、过期响应过滤、访问权限、HomeKit 独立控制器互操作、真实 TCP 配对、超过 1024 字节的配件数据库、事件通知及管理员权限。

自动化协议验证不等于每个型号的固件或 iPhone「家庭」实机验收。macOS 已实际检查界面、导入及家庭控制；Windows 已通过真实导入和台灯控制，iPhone 家庭配对尚未验收；Linux 桌面交互未验收。GPUI 0.2.2 的辅助功能支持也有限，后续升级框架时需单独检查 VoiceOver / 屏幕阅读器。

本次构建、交互与实机验证的具体边界见 [本地验证记录](docs/verification.md)。

## 许可

原创代码使用 MIT。第三方组件及移植来源见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 和 `licenses/`。本项目与小米、米家、Apple 无隶属关系，未获得 HomeKit / MFi 认证。
