# RuPost GitHub Actions 指南

RuPost 项目使用 GitHub Actions 来实现持续集成（CI）和持续交付（CD）。本文档系统说明了工作流的功能架构、触发机制、缓存策略与产物分发规范。

## 工作流概览

我们配置了两个核心工作流：

1. **CI (`ci.yml`)**: 负责代码质量规范检查、全平台单元与集成测试，并自动为分支和 PR 生成仅供开发者内部调试的调试版本（带完整符号表与行号）。
2. **Release (`release.yml`)**: 负责在版本发布时，自动编译剥离符号表（Strip Symbols）与开启 Thin LTO 优化的正式发行版二进制，并上传至 GitHub Releases。

---

## 1. 持续集成与开发者调试工件 (CI)

文件路径: `.github/workflows/ci.yml`

### 触发条件与并发控制
* **分支推送**: 推送到所有分支 (`branches: ['**']`)，自动忽略以 `v*` 开头的发布标签。
* **Pull Request**: 针对 `main` 分支的 PR 请求。
* **手动调度**: 支持通过 GitHub Actions 控制台手动派发 (`workflow_dispatch`)。
* **并发控制 (Concurrency)**: 当同一分支或 PR 连续推送提交时，自动取消上一次未执行完成的任务，避免无意义的算力排队。

### 包含的任务 (Jobs)
1. **Rustfmt**: 检查代码格式是否完全符合 Rust 官方规范。
   * 命令: `cargo fmt --all -- --check`
2. **Clippy**: 运行 Rust 静态代码分析工具。
   * 配置: 禁止警告 (`-D warnings`)，确保全工作区零警告通过。
3. **Test & Debug Artifact (多平台矩阵)**:
   * **矩阵平台**: `ubuntu-latest` (Linux), `macos-latest` (macOS), `windows-latest` (Windows)。
   * **性能与缓存加速**:
     * 显式注入 `CARGO_INCREMENTAL: 0`，消除 CI 环境生成增量元数据的磁盘 I/O 阻塞。
     * 配置 `Swatinem/rust-cache` 并开启 `cache-on-failure: true`，即使测试偶发失败也能锁住依赖缓存。
   * **测试执行**: 先执行 `cargo build --workspace --bins` 预热二进制，再执行 `cargo test --workspace`。
   * **调试版本工件直出 (Debug Artifact)**:
     * 直接复用当前机器刚刚编译好的 `target/debug/rupost`（或 `rupost.exe`），实现 **0 额外编译耗时**。
     * 产物保留完整 DWARF/PDB 调试符号与行号，供开发者在本地测试时配合 `RUST_BACKTRACE=1` 查看精准崩溃栈。
     * 存储于 Actions 运行记录的私有存储桶中（非公开 Release），配置 `retention-days: 7`，7 天后自动销毁，节约存储配额。

---

## 2. 正式版本自动发布 (Release)

文件路径: `.github/workflows/release.yml`

### 触发条件
* 推送以 `v` 开头的版本标签 (例如 `v0.2.0`, `v1.0.0`)。

### 生产环境编译优化与符号剥离 (Strip Symbols)
在发布阶段，工作流通过环境变量强制注入生产级优化，不侵入本地 `Cargo.toml` 开发配置：
* `CARGO_PROFILE_RELEASE_STRIP: symbols`: 自动剥离符号表与调试信息，二进制体积缩减 60% ~ 80%，且杜绝开发者本地源码路径外泄。
* `CARGO_PROFILE_RELEASE_LTO: thin`: 开启跨 crate 链接期死代码消除优化。
* `CARGO_PROFILE_RELEASE_CODEGEN_UNITS: 1`: 提升代码生成合并率。
* `Swatinem/rust-cache`: 针对不同 target 配置独立缓存键（`release-${{ matrix.target }}`），加速后续版本发布。

### 构建目标矩阵
* `x86_64-unknown-linux-gnu` (Linux x86_64)
* `x86_64-apple-darwin` (macOS Intel)
* `aarch64-apple-darwin` (macOS Apple Silicon)
* `x86_64-pc-windows-msvc` (Windows x64)

构建产物自动打包为 `.tar.gz` 或 `.zip` 格式并挂载至 GitHub Releases 页面，供全球用户公开下载。

---

## 3. 发布操作规范

在本地完成质量校验并准备发布时，执行以下命令：

```bash
# 1. 确保当前分支代码干净并已推送
git checkout main
git pull

# 2. 创建带注解的语义化版本 tag
git tag -a v0.2.1 -m "Release v0.2.1: Web workbench dual-theme support and stability hardening"

# 3. 推送 tag 触发自动发布流水线
git push origin v0.2.1
```

推送后前往 GitHub Actions 页面即可实时观察 Release 跨平台构建进度。
