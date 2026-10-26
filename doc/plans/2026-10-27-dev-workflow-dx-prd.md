# RuPost 开发流程体验完善 PRD (Dev Workflow DX PRD)

> **日期**: 2026-10-27 ｜ **状态**: 待实施 ｜ **事实源分工**: 需求与队列见 [主执行文档](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md)；本文件为 DX 批的 PRD 与执行计划
> **对象**: 开发 rupost 本身的体验（DX），不是产品 UX
> **模板依据**: [研发流程规范 §0](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md) 的 PRD 四段 + 用户故事 GWT

---

## 1. 背景与痛点（全部实证，非推测）

| # | 痛点 | 证据 |
| :-- | :--- | :--- |
| P1 | **无统一任务入口**：提交前三步门禁 + 两个回归脚本 + 覆盖率全靠手敲长命令；本项目无 Makefile/justfile/.cargo alias | `ls Makefile justfile .cargo` 均无；测试方案 P2 早有"覆盖率固化为 make/脚本入口"待办 |
| P2 | **门禁靠记忆执行，事后才被打回**：2026-10-27 会话实现 1a/1b 时，fmt 漏跑 1 次、clippy 抓出 `unused-mut` 与 `cloned_ref_to_slice_refs` 各 1 次——全部由事后门禁兜住而非流程预防；一条聚合命令可显著降低"跳过检查"的诱惑 | 本会话 jj 提交 `340bd0cf` 前的门禁记录 |
| P3 | **流程规范自身漂移**：development_workflow §6 指向**不存在的** `doc/progress_summary.md`（现行真身在 `doc/plans/`，同名旧快照在 `doc/dev/`——与审计 P0-2 同源）；§4 第 3 步仍教手动双终端 Mock 流程，而 `examples/run_all.sh` 早已自动化；checkpoint.md 角色双写（流程规范说"每 7 次对话总结"，实际文件是质量加固归档） | 本会话 Read 核实 |
| P4 | **新文档制度无模板承载**：状态横幅、事实源分工、PRD 四段、US-GWT 全是文字规范，落地靠记忆——正是 27 份悬空设想无横幅的根因之一 | [完整开发方案 §3.3](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-complete-development-scheme.md) |
| P5 | **本机环境坑无固化入口**：`SDKROOT=.../MacOSX26.5.sdk` 规避 CommandLineTools 27.0 的 .tbd 损坏，只埋在 progress_summary:188 行的括号里，新会话/新机器必踩 | progress_summary TUI Phase 1 回归记录 |
| P6 | **覆盖率未常态化**：测试方案 P2 要求"本地可选命令固化为 make/脚本入口；暂不进 CI 门禁" | [测试方案 §6](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md) |

## 2. 目标与非目标

**目标**
- G1 单一任务入口：根目录 `Makefile`，一条命令完成门禁、回归脚本、脚手架、覆盖率；
- G2 流程规范修正批：development_workflow 三处漂移修正（路径 / run_all.sh 引用 / checkpoint 角色澄清），并入[完整开发方案 §3.3 的文档治理横幅批](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-complete-development-scheme.md)同一原子提交；
- G3 文档脚手架：`doc/templates/prd-template.md`（预置状态横幅、事实源分工、四段结构、US-GWT 骨架）+ `make prd NAME=foo` 一键建档；
- G4 环境坑固化：SDKROOT 处理进 `make build`（自动检测并以明确指引失败），workflow/AGENTS 不新增重复记录；
- G5 覆盖率入口：`make coverage`（cargo-llvm-cov，本地可选，维持不进 CI 门禁的既有裁决）。

**非目标**
- 不引入 just / cargo-make / cargo-nextest 等新工具依赖——`make` 为 macOS/Linux 内建；**Makefile 仅服务本地 DX，Windows CI 路径不依赖它**（CI 三 job 结构不动）；
- 不做 jj/git hook 强制门禁（jj 无 hook 生态，colocated 仓库 git hook 对 jj commit 不可靠）——纪律模型 = `make gate` 低摩擦 + CI 三平台兜底；
- 不改任何产品功能与测试基线。

## 3. 用户故事与验收标准（GWT）

**US1 提交前门禁**
> As a 开发者, I want to run `make gate`, So that 三步门禁一条命令完成且失败即刻停止。

- Given 工作区存在 fmt 违规，When `make gate`，Then 在 fmt 步骤以非零退出，不执行 clippy/test（fail-fast，省时）；
- Given 全部通过，When `make gate`，Then 依序输出三步各自成功。

**US2 核心引擎改动全量回归**
> As a 改 parser/executor 的开发者, I want `make gate-all`, So that 门禁 + `verify_features.sh` + `run_all.sh` 一键完成。

- Given 核心引擎改动已过 `make gate`，When `make gate-all`，Then 两脚本顺序执行且 15/15 示例全绿。

**US3 新功能立项建档**
> As a 立项者, I want `make prd NAME=foo`, So that 生成 `doc/plans/YYYY-MM-DD-foo-prd.md` 且自带状态横幅、事实源分工、四段结构与 US-GWT 骨架。

- Given 无同名文件，When `make prd NAME=foo`，Then 文件生成且占位符齐全；同名已存在时拒绝覆盖（复用 `init --force` 的安全分级先例）。

**US4 新机器/新会话构建**
> As a 换机的开发者, I want `make build`, So that SDK 环境坑自动暴露而非构建到一半报 .tbd 损坏。

- Given CommandLineTools 27.0 环境，When `make build`，Then 检测并提示 `SDKROOT=` 修正指引（或自动选用可用 SDK），不静默吞错。

**US5 覆盖率观察**
> As a 维护者, I want `make coverage`, So that 测试方案 P2 的覆盖率观察一条命令可执行。

- Given 已安装 cargo-llvm-cov，When `make coverage`，Then 输出汇总；未安装则打印一行安装指引。

## 4. 需求规格

### 4.1 功能需求（Makefile 目标规格）

| 目标 | 行为 | 对应痛点 |
| :--- | :--- | :--- |
| `make gate` | `fmt --check` → `clippy -D warnings` → `cargo test`，fail-fast | P1/P2 |
| `make gate-fast` | 仅 fmt + clippy（编辑中间态快速自检） | P2 |
| `make gate-all` | `gate` + `tests/verify_features.sh` + `examples/run_all.sh` | P1 |
| `make examples` / `make features` | 单独跑两个 L3 脚本 | P1 |
| `make prd NAME=<x>` | 生成日期前缀 PRD（拒绝覆盖） | P3/P4 |
| `make build` | SDK 检测 + `cargo build` | P5 |
| `make coverage` | llvm-cov 汇总（工具缺失给指引） | P6 |
| `make help` | 目标清单（注释即文档） | P1 |

### 4.2 非功能需求

- 幂等：所有目标可重复执行；
- `make gate` 在干净工作区全绿耗时 ≤ 现状三命令之和（零包装开销）；
- 零 Cargo 依赖变更；零 CI 变更；
- Makefile 内注释与 development_workflow 双向引用（入口即文档）。

### 4.3 成功指标

- 会话内"被门禁打回"次数归零（问题在写入时由 `gate-fast` 拦截）；
- development_workflow 0 错误路径（治理批验收）；
- 新建 PRD 100% 自带状态横幅与事实源分工字段；
- 新会话不再需要考古 progress_summary 才知道 SDKROOT 坑。

## 5. 执行计划（并入主执行文档队列）

| 卡 | 内容 | 量级 | 顺序 |
| :-- | :--- | :--- | :--- |
| DX-1 | Makefile 全目标落地（含 `make help`） | 0.5 天 | **下一个动作可与 T10 并行**（此后每个任务的门禁都受益） |
| DX-2 | development_workflow 三处修正——**并入文档治理横幅批**同一原子提交（治理批见完整开发方案 §3.3） | 0.25 天 | 随治理批 |
| DX-3 | `doc/templates/prd-template.md` + `make prd` | 0.5 天 | DX-1 之后 |
| DX-4 | `make build` SDK 检测（SDKROOT 逻辑从 progress_summary:188 升格为代码） | 0.25 天 | DX-1 之后 |
| DX-5 | （P2 可选）CI `workflow_dispatch` 手动触发跑 `run_all.sh` | 0.5 天 | backlog |

依赖关系：DX-1 是 DX-3/DX-4 的载体；DX-2 独立并入治理批；DX-5 独立。
