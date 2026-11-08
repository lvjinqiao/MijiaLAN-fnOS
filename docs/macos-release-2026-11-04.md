# macOS Rust 正式体验包 · 2026-11-04

此记录对应 Build 2。其面板外层矩形背景已在 [Build 3](macos-panel-surface-2026-11-04.md) 修复，后续体验应使用 Build 3。

- 产品：MijiaLAN；源码分支：`MijiaLAN-rs`。
- 版本：0.1.0，Build 2；Apple Silicon / arm64，Release 优化。
- App / 内置 CLI：Developer ID、Hardened Runtime、安全时间戳。
- App 公证：Accepted，`6a1d084e-3189-4740-8ef1-039d90648806`。
- DMG 公证：Accepted，`270151b0-e0bd-4529-895e-736a0a397c1c`。
- App 与 DMG 已附加票据，Gatekeeper 和 ZIP App 的 `syspolicy_check distribution` 通过。
- 36 项工作区 Release 测试通过；最终 ZIP 和只读挂载 DMG 的 App 文件内容一致。
- 最终包内许可证与源文件一致；应用资源中的 Cargo 许可证清单覆盖 634 个解析到的依赖包。

## 运行验证

ZIP App 已在当前 Mac 启动，演示页面正常显示并退出。随后通过同一 ZIP App 的普通模式观察到 11 台已有设备、2 台在线、台灯亮度 49%，设置页显示家庭桥已配对及 11 台配件；原生服务器菜单可展开和按 Escape 取消。未重新导入、重新配对或发起设备写入。验证后退出测试实例，原 Swift 进程保留。

许可证说明更新后重新签名、公证和核对最终安装包；GUI / CLI 的 Mach-O UUID 与交互测试时一致。原有 Rust 数据目录和标识 `xyz.ipocket.MijiaLAN.Rust` 保留；新签名首次读取旧凭据时仍可能出现系统授权提示。

## 文件

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| `MijiaLAN-rs-0.1.0-macos-arm64.dmg` | 15730284 | `e67bd349563a8818c123cc27165b1ae92215bb778c465e112b9c6bd8f9ec6f8e` |
| `MijiaLAN-rs-0.1.0-macos-arm64.zip` | 15608602 | `1f5b57c3b494daf8a4e3a9f3248238e02c8e5d780b16fc5e8ee1fe6dceefef65` |
| `SHA256SUMS.txt` | 200 | `6df3f1876f6c39bf76583361121a37ed30d09bb8750a399142df85eb5eecca90` |

构建、签名、公证、包验证与启动截图保存在本机 `build/releases/v0.1.0-rs.1/`，不提交个人运行截图、凭据或日志。分发文件位于其 `dist/` 子目录。本轮未发布 GitHub Release，未生成 Intel 包，也未再次测试 iPhone 家庭写入或所有设备型号。
