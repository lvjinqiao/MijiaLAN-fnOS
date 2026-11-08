# Rust CI 与发布

适用于 `MijiaLAN-rs`。

## 为什么 b11 慢

签名运行 `41964403656` 的日志显示 `No cache found`：Intel 宿主测试编译 11 分 48 秒，另行 ARM Release 编译 25 分 59 秒，签名、公证及封装约 2 分钟。新 tag 不能读取上一个 tag 的 GitHub 缓存。Windows 同次运行命中约 1.3 GB 缓存，测试编译约 39 秒、Release 编译约 6 分钟。缓存已开启，但命中范围、两套构建 profile 和宿主架构影响了效果。

## 新流程

1. **Build** 在 `MijiaLAN-rs` push / PR 时运行。Windows x64、Mac ARM64、Mac Intel 并行；Mac ARM64 使用原生 `macos-26`，采用单编译任务以适应标准 M1 runner 的 7 GB 内存。Intel 与 Windows 使用两个任务。
2. Clippy、测试及打包使用同一个明确的 native target 和 Release profile。首次预热仍要编译依赖；后续在同一分支复用依赖缓存。
3. 完整缓存保留 Cargo 的程序、测试二进制及 GPUI，避免依赖缓存的清理逻辑删除工作区产物。缓存键包含编译器、SDK、架构、编译环境、vendor Git tree 和完整编译输入内容哈希。只有已缓存的编译输入内容完全相同时才恢复源文件 freshness；源码或锁文件变化时保留正常重编译，并仅借用兼容的依赖缓存。PR 只读缓存。旧依赖缓存作为迁移预热种子，不额外重复保存。
4. 每个平台产出 ZIP、校验文件和 `build-info.json`，记录准确源码 SHA、架构、Release profile、测试数及载荷哈希。包内已经包含依赖许可；Windows 包含 VC++ 运行库安装说明。
5. **Signed desktop release** 等待该提交的 Windows / Mac ARM 构建成功，直接下载并核对这些程序。Mac 仅更新发行元数据、Developer ID 签名、公证和封装，不安装 Rust、不重复编译、不重复运行全工作区测试；会验证包内 CLI。临时签名材料始终清理，不参与缓存。
6. 汇总 macOS DMG / ZIP、Windows ZIP 和 SHA-256 清单。默认手动运行只生成预览 Artifact；显式选择发布或推送 rs tag 后，先创建 Release 草稿、上传及核对服务端哈希，再公开为 Rust prerelease，保留 Swift 的 Latest 标记。已公开的同名 Release 不会被覆盖。

## 操作

手动预览（不会创建新 tag 或公开 Release）：

```sh
gh workflow run release-macos.yml --repo iPocket-APP/MijiaLAN --ref MijiaLAN-rs \
  -f build_number=12 -f publish=false
```

确认产物后，可用新的 build 号并选 `publish=true`，或在已 push / 验证的提交上创建 `rs-vVERSION-bBUILD` tag。每个 tag 必须来自 `MijiaLAN-rs`，版本必须匹配对应提交的 Cargo 版本。`source_ref` 可指定包含本流程的 rs tag；`ci_run_id` 可锁定同一源码的 Build 运行。CI Artifact 保留 14 天；缺失或过期时需先重跑对应 Build。

只验证正式发布所需的两个平台，可手动运行：

```sh
gh workflow run ci.yml --repo iPocket-APP/MijiaLAN --ref MijiaLAN-rs -f platform=release
```

其他选项为 `all`、`windows`、`macos-arm64`、`macos-intel`。每个 Build job 的 Summary 显示精确缓存命中状态、target、profile 及测试数。不得把“缓存步骤执行成功”当作命中证明；首次预热、锁文件 / Rust / SDK / vendor 变动仍可能重新编译。

参考：[GitHub 缓存作用域](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)、[标准 runner 规格](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)、[rust-cache 配置](https://github.com/Swatinem/rust-cache)。
