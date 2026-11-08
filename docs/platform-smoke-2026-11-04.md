# 跨平台验收记录 · 2026-11-04

## 当前候选包

这些是 x86_64 Debug 测试包，未做 Windows 代码签名。以下哈希已在构建主机重新计算并与清单核对。

| 文件 | SHA-256 |
| --- | --- |
| `MijiaLAN-Rust-0.1.0-windows-x86_64-debug.zip` | `2da14d78f96b682ad917c2d431fbef392e3b2363ce580a91f87527077bf66147` |
| `MijiaLAN-Rust-0.1.0-windows-x86_64-debug.msi` | `2caa2f06ad972bb9883654da381d960052ef6e2408b3d8b4d8f76f7c05d59a74` |
| `mijialan-rust-0.1.0-linux-x86_64-debug.deb` | `003109f27963600fa7a95942519d0d2408ac134b8b8a59e49c168821af4e8f08` |
| `mijialan-rust-0.1.0-linux-x86_64-debug.tar.gz` | `eebcd3700db9073216c0df2e59ee648763d875f4dac39f3ffc19dd162e03954a` |

## Windows 实机边界

群晖 VMM 的 `win11`：Windows `10.0.26200.7840`，800×600，Microsoft 基本显示适配器。验证期间未关闭 Defender、设置排除项或修改系统防火墙。

### 此前的宽窗口包

以下步骤记录紧凑托盘版之前的安装与协议验收。其 GUI 哈希与截图不能代替下文当前候选包的验收。

1. 早期 NSIS EXE 被 Defender 标记为 `Trojan:Win32/Bearfoos.A!ml`，在运行前已被系统处置，未解除隔离，不能断言误报。该 EXE 不属于通过验收的分发入口。
2. 初版便携包核对哈希后解包，载荷自定义扫描无相关新增检测；CLI `--help` 退出码为 0。初版 GUI 返回 `0xC0000139`，定位到交叉构建缺失 Common Controls v6 清单。
3. 补齐 PE 清单，并将 Debug HLSL 改成嵌入后从内存编译。修复版 GUI 已在此 VM 实际显示中文设备页面，使用演示数据，默认台灯亮度 71%。载荷已核对哈希并完成自定义扫描，无相关新增检测。截图：`artifacts/evidence/windows-demo.jpg`。
4. 800×600 下旧默认窗口超出桌面；最终包加入小屏默认最大化及最小尺寸约束，实机已确认首次显示完整窗口、左侧设置入口及底栏。
5. 中途控制台自动输入停止响应，连 Windows 自带「运行」对话框也无法确认。用户手动恢复 PowerShell 后继续验证，没有据此修改客户端输入处理。后续控制台鼠标落点仍有偏移；用户在最终客户端手动点击「设置」后页面正常打开，截图：`artifacts/evidence/windows-final-settings.jpg`。控制台偏移的具体原因未定位，不能据此声称客户端鼠标存在缺陷。自动工具发送的 `Ctrl+Q` 实际打开了 Windows 搜索；改用 noVNC 自带的远端 Ctrl 按钮配合 Q，客户端成功退出。已确认远端 Ctrl 按钮释放；设置快捷键尚未单独验收。
6. 首次标准 MSI（SHA-256 `1878bcf4bdfedcd3c8dcbeb18971ac3204d65d32a6291831b4b62072ff7c5392`）已按当前用户安装，安装日志返回 0，正常及演示开始菜单入口均存在。发现 Debug GUI 的附加控制台会遮挡并抢占焦点，已为 Windows GUI 入口设置 `windows_subsystem = "windows"`。最终 GUI 的 PE 子系统为 2，CLI 为 3；最终 MSI 已完成同版本升级，日志返回 0。最终 GUI 实机 SHA-256 为 `abea1f7a2d70b61b319428dc68929f3c9adbaed524d4ca124dffcf860b23b0bc`，与 ZIP 中载荷一致。截图：`artifacts/evidence/windows-final-msi-installed.jpg`。
7. 两次 MSI 均在 VM 核对哈希并进行 Defender 自定义扫描，相关检测记录数量为 0；扫描时检查的防病毒与实时保护状态为开启，验证期间未设置排除项、解除隔离或关闭防护。最终包扫描截图：`artifacts/evidence/windows-final-msi-scan.jpg`。这只是本次环境的扫描结果，不代表第三方安全认证。
8. 安装版 CLI `--help` 正常显示，正常模式 `list` 输出空列表并返回 0。空清单不会触发凭据主密钥创建，因此这一步不能视为已验证 Windows Credential Manager 的设备凭据读写。
9. Windows 后端检查包完成哈希核对、解包与 Defender 自定义扫描，无相关检测记录；6 个测试可执行文件实际运行成功：4 + 4 + 0 + 8 + 7 + 10 = 33 项通过，批处理退出码 0。包含 miIO UDP、HomeKit 独立控制器配对、加密、事件及同端口重启。VM 原始日志：`%TEMP%\mijialan-checks.log`；截图：`artifacts/evidence/windows-backend-tests.jpg`。

此前宽窗口安装版已从开始菜单的演示入口实际启动，无附加控制台，中文首屏和设置页可见。随后正常退出演示实例，并从普通入口打开正常模式，显示 0 台设备；截图：`artifacts/evidence/windows-final-normal.jpg`。当时尚未验证 Windows 凭据读写、扫码与真实控制，后续结果见下文；演示设备不能当成真实导入记录。

MSI 是当前用户安装，不含自定义动作。已静态检查文件、升级与移除表；卸载不包含设备资料或凭据。安装日志位于 `%TEMP%\mijialan-gui-install.log`。安装包内附的 `verification.md` 为打包时的状态，当前文件记录后续实际进度。

### 当前紧凑托盘版

按用户要求改用 Microsoft Remote Desktop（应用显示名 Windows App）连接同一台 Windows 11 虚拟机，不再通过 VMM 控制台操作 Windows。RDP 会话的桌面在测试中动态调整尺寸，应用日志记录 96 DPI、面板宽 360 像素，定位到工作区右下角。自动输入在 Scancode 模式下丢失符号，切到 Unicode 后下载、检查和安装命令正常执行；远程工具的鼠标坐标偏移仍需区别于应用自身的点击处理。

此前托盘 MSI（`a8bc952eb1e81414c5ef402664df03bf715adce2119bfeeebb42045f020d0dbc`）已在 Windows 核对哈希，完成 Defender 自定义扫描，相关检测数为 0；同次验证期间检查的防病毒和实时保护均开启。MSI 同版本更新返回 0，安装后 GUI SHA-256 为 `f4409be61dd3e7a83c523e24cf1ba62bc34f008a271df960211f30290d0dc1bd`，与当时 ZIP 中载荷一致。证据：`artifacts/evidence/windows-tray-resident-scan-rdp.jpg`、`windows-tray-resident-installed-rdp.jpg`。

紧凑界面已实际显示中文设备卡片、设置和刷新图标、完整离线网格与四角圆角。它使用独立演示数据（2 台在线、共 11 台），不代表 Windows 导入了真实设备。测试发现 GPUI 关闭最后一个窗口会无条件退出 Windows 消息循环，已修复为由应用决定退出：Escape 收起并释放视图后，PowerShell 查询同一个演示进程 `HasExited=False`，错误日志为空。证据：`artifacts/evidence/windows-tray-resident-demo-rdp.jpg`、`windows-tray-resident-background-rdp.jpg`。

用户随后在 Windows 的隐藏托盘图标中点击房屋图标，确认能重新打开面板；RDP 截图已实际观察到同一演示界面重新显示，并保留在前台供检查。最终截图：`artifacts/evidence/windows-tray-final-rdp.jpg`。EXE 静态资源检查也确认图标组 ID 为 1、包含 9 种尺寸，GUI 子系统为 2。

本轮 Windows 交叉构建成功；Mac 桌面目标 Clippy 与格式检查通过；3 项定位测试覆盖小屏高度、负坐标显示器、高 DPI、左侧和顶部任务栏。前述 Windows 33 项协议测试未因本次 UI 改动重复运行，核心和 HAP 源码未修改。后续真实扫码、凭据写入和菜单排查见下文；Windows 的 iPhone 配对尚未验收。

本机构建期间 Docker `run` 遇到缺失 stargz snapshot；一次空闲 Docker Desktop 重启没有解决。最终通过 BuildKit 编译并本地导出产物完成构建，没有清理全局缓存或重设 Docker、NAS、虚拟机配置。

### 真实控制模式排查

用户反馈界面按钮会变化，但真实设备无反应。RDP 中检查运行命令确认带有 `--demo`；核心代码在此模式跳过 miIO 网络写入，11 台设备来自内存夹具。切换普通模式后设备数为 0，说明 Windows 需要单独导入；未复制 Mac 凭据。

实测从空设备页打开设置后，下半页被旧视口截住。窗口改高时同步调用 `SetWindowPos`，会在当前 GPUI 窗口借用期间重入尺寸回调；已改为前台异步执行，并刷新最终视口。标题和底栏同时补充演示模式说明。

此前改高修复 MSI（`c8a50512bd7b0979c925a4cc675b42945d16b0a7a3f6fd2d3191fc747049b589`）已在 VM 核对哈希、自定义扫描，相关检测 0，防病毒和实时保护均开启；同版本升级返回 0。安装 GUI SHA-256 为 `63b64f52ca726ef7b5897f91a319f9aee81134c6fc7e75778a0e0a54cafea2d0`，与当时 ZIP 一致。证据：`artifacts/evidence/windows-real-mode-scan.jpg`、`windows-real-mode-installed.jpg`。Windows 交叉构建、Mac 目标检查与格式检查通过。

安装后已通过 RDP 实际点击空设备页的「导入设备」进入设置，整页与底部均完整显示；继续点击「扫码导入设备」进入登录页。日志记录窗口尺寸为设置页 360×634、空设备页 360×290，页面完整性以实际截图为准。证据：`artifacts/evidence/windows-real-mode-settings-fixed.jpg`。

第一次二维码请求报网络错误；Windows 自带 curl 访问同一服务返回 HTTP 200，随后在应用中重试成功显示二维码，具体瞬时失败原因未定位。用户手机扫码确认后，Windows 已保存 11 台真实设备；GUI 重启后读到 11 台，其中台灯和空调插座在线，验证了本机凭据写入与重新读取，无需复制 Mac 凭据或再次扫码。

随后用安装版 CLI 在真实模式读取 `yeelink.light.lamp4`，返回电源开启、亮度 49%、色温 2812 K。通过局域网将亮度属性 `2.2` 写为 50，设备确认、退出码 0；独立 `refresh` 读回 50。再写回原值 49，设备确认、退出码 0，重新读取为 49。证据：`artifacts/evidence/windows-real-lamp-cli-readback.jpg`。这是 Windows 的真实 LAN 应答和读回验证；其他离线设备及 Windows iPhone 配对仍不在通过范围内。

在首轮菜单修复版中，RDP 实际拖动 GUI 台灯亮度条，从 49% 调到 54%；另起安装版 CLI 刷新设备读回 54。随后 LAN 写回 49，设备确认、退出码 0，再次独立刷新读回 49。证据：`artifacts/evidence/windows-real-lamp-gui-readback.jpg`。这一步验证了界面到实际设备的写入链路。

### 原生下拉菜单崩溃

用户报告闪退后，GUI 标准错误记录 `vendor/gpui/src/app/async_context.rs:57:27: RefCell already borrowed`。开启 `RUST_BACKTRACE=1` 重启后，通过 RDP 实际点击设置中的服务器下拉菜单，稳定复现退出。调用栈依次包含 `TrackPopupMenu`、`show_popup_menu`、`App::apply_defer_effect`，嵌套消息循环中运行 `BlinkCursor::blink` 并再次借用 `AppCell`。证据：`artifacts/evidence/windows-import-crash-before-fix.jpg`、`windows-native-menu-crash-stack.jpg`。

`App::defer` 仅保证实体归还，执行时仍持有 App 借用。修复将原生菜单跟踪直接安排到前台执行器，跟踪期间不持有 App 或 Window 借用；菜单返回后才用异步上下文应用选项，并确认原窗口仍存在。Mac 菜单实现和设备协议未修改。

首轮菜单修复 MSI（`fd50efdeef019cdd0755e97413e6067603404e08b330b9baad2359b4c5da1484`）在 Windows 核对 SHA-256 后完成 Defender 自定义扫描，相关检测 0，防病毒与实时保护开启；同版本更新返回 0。安装后的 GUI SHA-256 为 `325d5b4c5e2ecb134433241d8fa8c801bee290288e2a4fe80035243966bf6456`，与当时 ZIP 载荷一致。GUI 普通模式启动后仍显示 11 台设备、2 台在线和台灯亮度 49%。证据：`artifacts/evidence/windows-menu-fix-scan.jpg`、`windows-menu-fix-installed.jpg`。该版本并未通过完整菜单验收，取消操作的问题见下文。

首轮修复后，原生菜单可持续展开并选择首项，但按 Escape 取消后，进程错误日志显示 `windows_panel.rs:217:44: attempt to subtract with overflow`。原代码 `(chosen > 0).then_some(chosen as usize - 1)` 会提前计算参数，即使取消返回 0 也先执行减法。修复改为独立的安全命令解码：先转换非负编号，再用 `checked_sub(1)`，并检查选项范围。2 项回归测试覆盖取消、负值、超范围、空菜单、首项、中项和末项，Debug 下通过。证据：`artifacts/evidence/windows-native-menu-cancel-crash.jpg`。

最终 MSI（本文开头的 `2caa2f06…`）已在 Windows 核对完整 SHA-256，Defender 自定义扫描完成，相关检测 0，防病毒与实时保护均开启；同版本升级返回 0。安装后的 GUI SHA-256 为 `d3cc8551c31eb042c3744d5f67458f0111a05098252bc155a70a370f03902ff1`，与最终 ZIP 载荷一致。普通模式启动保留 11 台真实设备、2 台在线及台灯亮度 49%。证据：`artifacts/evidence/windows-menu-final-scan.jpg`、`windows-menu-final-installed.jpg`。

最终安装版通过 RDP 连续完成 4 次原生菜单展开和关闭：持续展开约 20 秒、Escape 取消、选择「欧洲」并核对勾选后恢复「中国大陆」、点击应用外部取消。日志中的菜单打开和关闭成对出现；取消后选项保持原值。随后再次验证面板 Escape 收起、外部点击收起，以及点击房屋托盘图标重新创建面板；设置页和设备页均正常恢复。同一进程 PID 6992 仍在运行，最终检查 `Running: True`、标准错误文件 `ErrorBytes: 0`。本轮未再复现两种崩溃，未进行长时间稳定性测试。证据：`artifacts/evidence/windows-menu-final-selection.jpg`、`windows-menu-final-cancel-check.jpg`、`windows-menu-final-verified.jpg`。最终停留在真实设备页供用户继续操作，无需重新扫码。

旧 Debian 构建层报告缺失父快照，首轮菜单修复改用新的 Ubuntu 24.04 构建环境完成。取消修复再次遭遇缓存父快照错误，最终采用官方 `rust:1.98-slim-bookworm` 新构建层，Rust / MinGW 完整编译（7 分 17 秒）、打包和本地导出成功，ZIP 完整性与 SHA-256 清单检查通过；没有清理或重设全局 Docker 状态。Mac 目标检查和格式检查通过。Linux 分发包保持原有 Debian 12 验证边界，未由此重建 Linux 包。最终 MSI 的临时单文件传输服务已在 Windows 下载核对后停止。

## Linux 验证边界

Debian 12 x64 构建容器成功生成 GUI 和 CLI。共享核心 14 项、HomeKit 19 项测试全部通过，包括 UDP 模拟设备与独立加密 TCP 控制器。CLI 演示模式通过，ELF 依赖均可解析。干净 Debian 12 slim 容器成功解析 DEB 依赖、安装，并运行 CLI `--help`。

尚未在真实 Linux 桌面验证窗口、Vulkan、托盘、Secret Service、扫码和真实设备。此包要求 glibc 2.36 或更新；NAS 中原有 Debian 11 / Ubuntu 20 虚拟机不作为这批包的兼容目标，未启动或修改它们。

## Apple TV

当前 GPUI 0.2.2 不提供 tvOS 后端，本轮没有电视安装包。需要独立 tvOS 界面和平台适配；Rust 具有 tvOS 编译目标本身不足以让此桌面客户端运行。

## 本机保留

原 Swift 项目未修改，本机正在运行的 Swift / Rust 客户端未替换或退出。本轮容器和 Windows 包未包含 Mac 的凭据或桥接身份。

本轮最终验收后，临时局域网传输服务已停止，临时下载别名已移除，VNC 缩放已还原为 Local Scaling。旧 NSIS EXE 保留于 `artifacts/rejected/`，仅供排查；中间构建包移至 `artifacts/iterations/`。未来如需传输，应重新建立仅用于候选包的传输入口。
