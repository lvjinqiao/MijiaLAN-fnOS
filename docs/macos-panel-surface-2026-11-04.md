# macOS 面板外层黑角修复 · 2026-11-04

## 原因与修复

GPUI Component 0.5.1 的 Root 会铺满一个矩形主题背景，而面板内层的圆角及矩形 overflow clip 无法裁掉这个背景。Build 3 将 NSWindow 设为非不透明、清空窗口底色，并在原生内容视图层设置 18 pt 圆角与 `masksToBounds`，覆盖全部 Metal / GPUI 子层。创建和内容改高后重新应用裁剪，并刷新系统阴影；未修改设备、凭据或家庭桥协议。

## 验证

- 隔离演示包检查浅色 / 深色的设备页、设置页及页面往返，均没有原来的黑色直角背景。
- 设备页 494 pt 与设置页 634 pt 改高后，日志均为 `surface opaque=false radius=18 clipped=true`。
- 最终 ZIP 中的 Release App 实际启动，深色设备页和设置页完整圆角可见；演示模式未读取真实凭据或写设备。
- 36 项工作区 Release 测试、桌面 Clippy 与格式检查通过。
- App / CLI 的 Developer ID、Hardened Runtime、时间戳与签名验证通过。
- App 公证 Accepted：`6acf4be7-ede4-42ce-9bc0-720c4bed1b08`。
- DMG 公证 Accepted：`d68ffd6a-364e-406d-85f6-e1c11b7e268f`。
- 公证票据、Gatekeeper、ZIP 的分发检查、DMG 校验及挂载通过；ZIP 与 DMG 中 App 文件内容一致。
- 本轮只保留本地修复，用户要求暂停提交后未再创建提交或推送；尚无远端 Rust 分支更新。

## 体验包

版本 0.1.0 / Build 3，Apple Silicon / arm64。文件名包含 build，保留旧包供对照。Rust 的数据标识和目录不变。

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| `MijiaLAN-rs-0.1.0-b3-macos-arm64.dmg` | 15935036 | `585b1313a7de0518b723f95ccd582099c057c51a4036a0541b370ff402f6746f` |
| `MijiaLAN-rs-0.1.0-b3-macos-arm64.zip` | 15609993 | `846ec8cebbb34fc9b2eff4f41bc9b4b8edf71c7dba6a582de1d5a1b8ec791f10` |
| `SHA256SUMS.txt` | 206 | `ad0c52cf52ff8d2167e15c1ff77a5d17767423542f1313b8cd62284115f6a844` |

本机分发目录为 `build/releases/v0.1.0-rs.3/dist/`，截图、日志和校验报告也位于已忽略的 `build/`。安装包、缓存、编辑器目录、日志、截图及凭据不进入候选源码清单。原 Swift 客户端进程保留，所有新增的验证实例已结束。
