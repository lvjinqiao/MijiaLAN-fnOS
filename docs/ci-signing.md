# GitHub Actions macOS 正式签名

普通 `ci.yml` 执行 macOS / Windows 构建验证。`release-macos.yml` 单独处理 Apple Silicon 正式签名、公证和 DMG / ZIP 封装，不在 pull request 事件中运行，也不自动创建 GitHub Release。

## Environment

仓库 `iPocket-APP/MijiaLAN` 使用 `macos-release` 环境，只允许 `MijiaLAN-rs` 分支和 `rs-v*` 标签。环境规则可以预先存在，匹配的分支和标签需要实际推送后才能使用。

| 配置类型 | 名称 | 用途 |
| --- | --- | --- |
| Secret | `MACOS_CERTIFICATE_P12_BASE64` | 包含 Developer ID Application 私钥的加密 P12，转 Base64 |
| Secret | `MACOS_CERTIFICATE_PASSWORD` | P12 加密密码 |
| Secret | `APPLE_API_KEY_P8_BASE64` | App Store Connect 团队 API Key，转 Base64 |
| Variable | `MIJIALAN_SIGNING_IDENTITY` | 对应证书的 SHA-1 指纹或名称 |
| Variable | `APPLE_API_KEY_ID` | 团队 API Key ID |
| Variable | `APPLE_API_ISSUER_ID` | 团队 API issuer UUID |

私钥只写入环境 Secrets；Git 中保留脚本和说明。`gh secret set --repo iPocket-APP/MijiaLAN --env macos-release NAME` 可从标准输入接收值，并在发送前加密。不要将秘密作为命令行字面量、日志或 Actions artifact 保存。Team API Key 按角色可访问整个团队的应用，不能按单个 App 隔离；CI 专用 Key 应选择足够完成公证的权限，并独立管理和撤销。

## 运行方式

手动运行选择 `MijiaLAN-rs` 分支及正整数 build 号。GitHub 要求带 `workflow_dispatch` 的 workflow 文件位于默认分支，才提供对应手动入口；仅在 Rust 分支保留 workflow 时可使用标签事件触发。

标签格式为 `rs-v<version>-b<build>`，例如 `rs-v0.1.0-b4`。workflow 核对 Cargo 版本、build 号以及提交是否属于 `origin/MijiaLAN-rs` 历史，在导入密钥前拒绝不匹配的版本或来源。

Runner 在 `$RUNNER_TEMP` 创建独立临时钥匙串，导入指定证书，并检查配置的签名身份。公证 P8 文件的权限受 `umask 077` 保护。`package-macos-release.sh` 在 CI 使用 `APPLE_API_KEY_PATH`、`APPLE_API_KEY_ID`、`APPLE_API_ISSUER_ID`；本地仍支持 `MIJIALAN_NOTARY_PROFILE`。

为避开 GitHub 托管 Apple Silicon Runner 的容量限制，CI 使用 `macos-26-intel` 和 Xcode 26.3，安装 `aarch64-apple-darwin` 标准库，并通过 `MIJIALAN_MACOS_TARGET` 生成 arm64 App。工作区测试在 Intel 宿主机执行，目标产物另做架构、签名和公证校验；arm64 安装包的运行验证需在 Apple Silicon Mac 上完成。

正式签名 workflow 也支持本机 Apple Silicon Runner：在仓库级 Actions variable `MIJIALAN_MACOS_RUNNER` 中填写专用 Runner label；未设置时仍使用上述托管镜像。该变量需要在触发构建前配置，不能放在 Environment variables 中。可使用带独立 label、`--no-default-labels` 和 `--ephemeral` 的临时 Runner，只执行一次任务后自动注销。构建结束后删除临时变量，恢复托管机器默认值；不需要安装常驻服务。本机需预装 Rust 1.98.1、rustfmt、clippy 和可用的 Xcode / Metal 工具链；workflow 使用当前选中的 Xcode，通过 `DEVELOPER_DIR` 指定，不修改系统 Xcode 选择，也不安装或切换本机 Rust。已有 Cargo 缓存可以通过 Runner 进程的 `CARGO_TARGET_DIR` 复用，目录应位于 Git 忽略范围内。

Runner 的安装路径和工作目录应不含空格，避免 shell 脚本路径解析失败。正式封装会在签名前将 App 目录和可执行文件规范为 `755`、普通资源文件规范为 `644`，并设置 DMG 根目录和说明文件的读取权限；Runner 的私有 `umask` 不会进入分发包。签名私钥所在的临时目录仍使用独立的严格权限。

普通矩阵顺序执行，先运行 Windows 检查。托管 macOS 作业与正式打包共享 `mijialan-macos-hosted` concurrency group，在调度层等待，而不同时申请多台 Mac Runner。使用本机专用 label 时，正式打包使用该 label 作为独立 concurrency group，避免等待托管机器。

流程依次运行工作区测试、Release 构建、Developer ID / Hardened Runtime 签名、App 公证与票据附加、DMG 公证与票据附加及 Gatekeeper 检查。托管 Intel 宿主机测试使用 Debug 配置，本机 Apple Silicon Runner 使用 Release 配置；分发二进制均使用 arm64 Release 配置。最终 DMG、ZIP、SHA-256 清单上传到本次运行的 Actions artifact。这些文件在 Runner 上生成，不进入源码 Git 历史。

附加和验证公证票据会对临时网络失败做最多六次重试，单次等待不超过 30 秒，最终失败仍使 workflow 失败。Rust 缓存在打包失败后也会保存。如果已完成签名的 App 存在，失败时另保留一个带校验清单的 recovery artifact 三天，便于重试封装；其中不包含钥匙串、公证私钥或日志。

无论打包结果如何，清理步骤恢复钥匙串搜索列表、删除临时钥匙串和私钥文件。环境、凭据和脚本已配置不等于 CI 已通过，必须另行完成一次真实 workflow 运行。密钥授权和源码提交按用户当前授权范围执行。
