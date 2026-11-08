# 把源码和 FPK 发布到 GitHub

建议使用 GitHub Desktop 上传源码，GitHub 网页上传安装包。源码放仓库；FPK 放 Releases。

## 1. 上传源码

1. 安装 [GitHub Desktop](https://desktop.github.com/)，登录自己的 GitHub 账号。
2. 使用交付目录里的 `GitHub/MijiaLAN-fnOS` 源码文件夹。这是干净副本，不含原仓库 `.git`、账号数据、测试日志或编译产物。
3. 在 GitHub Desktop 选择 **File → New Repository**。
4. Name 填 `MijiaLAN-fnOS`，Local path 选择上述 `GitHub` 父文件夹，使最终路径正好落在已有的 `MijiaLAN-fnOS` 文件夹。不要再生成一层同名子文件夹；不要另选许可证覆盖已有 LICENSE。
5. 点击 **Create Repository**。如有未提交文件，在左下角 Summary 写 `MijiaLAN fnOS 0.1.1 preview`，再点 **Commit to main**。
6. 点 **Publish repository**，确认账号和仓库名，取消 **Keep this code private** 即公开，最后点击 **Publish Repository**。

请保留 LICENSE、THIRD_PARTY_NOTICES.md、licenses/ 和 README 中上游来源说明。此项目是上游 MijiaLAN 的飞牛衍生版本。

## 2. 发布 FPK 下载

1. 打开刚发布的 GitHub 仓库，点击右侧 **Releases → Create a new release / Draft a new release**。
2. 新建 tag：`fnos-v0.1.1-preview`，Target 选择刚上传代码的 `main` 分支。
3. 标题填 `MijiaLAN 0.1.1 · fnOS x86_64 测试版`。
4. 把 `RELEASE-NOTES-0.1.1.md` 的内容复制到说明框。
5. 在附件区域拖入：
   - `MijiaLAN-fnOS-0.1.1-x86_64-preview.fpk`
   - `MijiaLAN-fnOS-0.1.1-source.zip`
   - `MijiaLAN-video-sources-0.1.1.tar.gz`（随视频程序分发对应源码）
   - `SHA256SUMS`
6. 勾选 **This is a pre-release**，点 **Publish release**。

之后把这个 Release 页面链接发给朋友，对方从 Assets 下载 `.fpk`，到飞牛应用中心手动安装。源码 ZIP 不是安装包。

当前尚未完成飞牛真机和全部 iPhone 配对验收，发布说明中保留测试版标识。不要上传工作目录 `work/`、`ui-data-v011/`、运行数据或整台电脑项目目录。

## 官方操作说明

- [创建本地仓库](https://docs.github.com/en/desktop/adding-and-cloning-repositories/creating-your-first-repository-using-github-desktop)
- [发布现有项目](https://docs.github.com/en/desktop/adding-and-cloning-repositories/adding-an-existing-project-to-github-using-github-desktop)
- [创建 Release 和添加附件](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository)
