# RuPost GitHub Actions 指南

RuPost 项目使用 GitHub Actions 来实现持续集成（CI）和持续部署（CD）。本文档介绍了现有工作流的功能、触发机制以及维护方法。

## 工作流概览

我们主要配置了两个工作流：

1.  **CI (`ci.yml`)**: 负责代码质量检查和测试。
2.  **Release (`release.yml`)**: 负责自动构建和发布二进制文件。

---

## 1. 持续集成 (CI)

文件路径: `.github/workflows/ci.yml`

### 触发条件
*   推送到 `master` 分支。
*   针对 `master` 分支的 Pull Request。
*   手动触发（Workflow Dispatch）。

### 包含的检查任务
该工作流包含三个并行或依赖的作业（Jobs）：

1.  **Rustfmt**: 检查代码格式是否符合 Rust 标准。
    *   运行命令: `cargo fmt --all -- --check`
2.  **Clippy**: 运行 Rust 静态分析工具，检查潜在的代码问题。
    *   配置: 禁止警告 (`-D warnings`)，确保代码干净。
3.  **Test**: 在多平台运行构建和测试。
    *   **矩阵策略**: 同时在 `ubuntu-latest`, `macos-latest`, `windows-latest` 上运行。
    *   **步骤**:
        *   安装 Rust 工具链。
        *   利用 `Swatinem/rust-cache` 缓存依赖，加快构建速度。
        *   执行 `cargo build` 验证编译。
        *   执行 `cargo test` 运行单元测试和集成测试。

### 如何查看结果
在 GitHub 仓库的 **Actions** 标签页中，点击最近的运行记录即可查看每个步骤的日志。如果构建失败，可以查看详细的错误输出进行修复。

---

## 2. 自动发布 (Release)

文件路径: `.github/workflows/release.yml`

### 触发条件
*   推送以 `v` 开头的 Tag (例如 `v0.1.0`, `v1.0.0`)。

### 自动化流程
当您推送一个新的版本 Tag 时，该工作流会自动执行以下操作：

1.  **创建 GitHub Release**:
    *   在 GitHub Releases 页面创建一个新的草稿或正式 Release。
    *   自动生成基础的 Release 说明（也可以配置从 CHANGELOG.md 读取）。

2.  **多平台构建与上传**:
    *   它会启动一个矩阵构建，针对以下目标平台编译 `rupost` 二进制文件：
        *   `x86_64-unknown-linux-gnu` (Linux)
        *   `x86_64-apple-darwin` (macOS Intel)
        *   `aarch64-apple-darwin` (macOS Apple Silicon)
        *   `x86_64-pc-windows-msvc` (Windows)
    *   构建完成后，会自动将二进制文件打包（tar.gz 或 zip）并上传到刚才创建的 Release 资产中。

### 如何触发发布

在本地终端执行以下 git 命令即可触发发布流程：

```bash
# 1. 确保当前代码是干净的并已推送到 master
git checkout master
git pull

# 2. 创建一个带注解的 tag (建议遵循语义化版本)
git tag -a v0.1.0 -m "Release v0.1.0: 初始版本"

# 3. 推送 tag 到远程仓库
git push origin v0.1.0
```

推送成功后，前往 GitHub Actions 页面，您应该能看到名为 "Release" 的工作流正在运行。完成后，在仓库主页右侧的 "Releases" 栏目中即可看到新发布的版本和下载链接。

---

## 维护与故障排查

### 常见问题

1.  **编译在 CI 失败但在本地成功**
    *   **原因**: 可能是因为 OS 差异（如 Windows 文件路径、行尾符）或缺少系统依赖。
    *   **解决**: 查看 CI 日志中的具体报错。如果是 Linux 上缺少库（如 OpenSSL），可能需要在 `ci.yml` 中添加 `sudo apt-get install ...` 步骤。

2.  **依赖缓存失效**
    *   我们使用了 `Swatinem/rust-cache`。如果发现构建变慢，可以尝试在 Actions 页面手动清理缓存，或者检查 `Cargo.lock` 是否发生了较大变动。

3.  **Release 权限错误**
    *   如果 Release 工作流提示 "Permission denied"，请检查仓库设置：
        *   Settings -> Actions -> General -> Workflow permissions
        *   确保勾选了 **Read and write permissions**。

### 扩展建议

*   **代码覆盖率**: 未来可以引入 `tarpaulin` 或 `llvm-cov` 并上传到 Codecov。
*   **Musl 构建**: 如果需要完全静态链接的 Linux 二进制文件（兼容 Alpine 等），可以调整 Release 流程使用 `cross` 编译 `x86_64-unknown-linux-musl` 目标。
