# Rupost 全功能域规范走查与测试完备性审计报告

> **文档状态**: Final  
> **审计日期**: 2026-11-09  
> **审计依据**: [开发规范 v1](development_standards.md) 八章节（提交/迁移/测试/编码/行为变更/安全/审查/门禁）  
> **审计方法**: 机械盘点（测试分布/to_str 残留/大小写口径/unwrap 热点全量扫描）+ 定点读码甄别（守卫 unwrap 可达性/子模块 import 依赖/零测文件清单）+ 既有五轮审查结论复用  
> **基线**: trunk `@`（895 passed / 0 failed，clippy/fmt 双零）

---

## 一、总体结论

**无 P0。规范违规仅 §2.2（glob 依赖 ×3，均为有意设计）与 §4（to_str 丢值 9 处，已全部登记 backlog）两项，且均有处置记录。测试完备性总体水位高（执行核心/服务群 8-9 分），结构性缺口集中在三处：Web 前端 JS 零测试基建、TUI 三个文件零直测（event/integrations/ws_interactive）、utils 尾部（formatter 0 测）。**

分域走查未发现假守护测试（抽查的 5 组关键守护——cookie 持久化分档、SSE 断流、选择器语义、adversarial panic 锚点、exporter 转义——删实现均变红）。

---

## 二、测试完备性分布（机械盘点实测）

### 2.1 各域测试数量（src 内 `#[test]` 计数 + tests/ 关联面）

| 域 | src 内测试 | tests/ 关联 | 评分 | 判读 |
| :--- | ---: | :--- | :---: | :--- |
| ai | 59 | 42 文件 | 9 | 全仓最厚实（B4 链/归因/聚类/bootstrap 全覆盖） |
| parser | 58 | 22 | 9 | 含 adversarial panic 锚点；metadata.rs 33 个 unwrap 全在 tests |
| runner | 52 | 22 | 8 | 协议执行器经 e2e 间接覆盖 |
| assertion | 50 | 17 | 9 | 最左扫描/None 语义决策表齐全 |
| tui | 104 | smoke 52 + features 8 | 8 | Phase 1-4 后各块均衡；缺口见 §三 |
| variable | 43 | 20 | 9 | 级联优先级决策表在（variable_integration_test:79） |
| http | 49 | 43 | 8 | SSE 1MB/llm/诊断 JSON schema 锁定 |
| assertion/middleware | 50/6 | — | — | cookie 25 个 unwrap **全在 tests**（生产用 into_inner 恢复）✓ |
| exporter | 33 | shell 7 测 | 9 | Pastejacking 防线有专测 |
| importer | 31 | 子进程 6 | 9 | 非 ASCII 往返守护已补 |
| report | 13 | 15 | 9 | XSS/schema/瀑布三线锁定 |
| mock | 12 | 34 | 8 | trie {id}/变体/双角色守护 |
| history | 10+2 | 14 | 8 | serialization 含损坏字节锁定（新增） |
| generator | 8 | — | 7 | trace 6/dynamic 2/http 0 直测（经 e2e） |
| web | 3 | 17 | 9(API)/3(JS) | API 侧安全系列齐全；**前端 JS 零基建** |
| template | 0 | 10 | 7 | 经 CLI 覆盖；strategy 无直测 |
| utils | 3+2 | 0 | 6 | jsonpath 2/path_guard 3/text 2/**formatter 0** |

### 2.2 规范违规清单（全部已有处置）

| 章节 | 违规 | 处置状态 |
| :--- | :--- | :--- |
| §2.2 glob 依赖 | `state/{stream,history,tests}.rs` 的 `use super::*`（3 处，有意设计） | P3：建议显式化（stream.rs 的 HashMap 已显式） |
| §4 to_str 丢值 | 9 处（trace×2/run/history/ws_session/ws_runner/replayer/axum_adapter/server/serialization） | 全部登记 future_backlog；serialization 已修 |
| §3.4 守卫 unwrap | `parser/metadata.rs:148` `find("from").unwrap()` | 前置 `from_keyword != "from"` 检查保证不变量——**守卫 unwrap（可接受）**，P3 建议改 let-else |
| §5 行为变更同步 | — | 无发现（本会话行为变更均三件套齐全） |
| §6 安全姿态 | — | 无发现（web 五暴露面 + mock 127.0.0.1 绑定均有测试锁定） |
| §1 提交纪律 | — | 会话内 1 次快照吸收（已 split 修复）+ 1 次哈希错指（已勘误）——规范即因此而生 |

---

## 三、测试完备性缺口清单（按优先级）

### P1（结构性缺口，建议立项）

1. **Web 前端 JS 零测试基建**（`src/web/assets/app.js` ~700 行）：
   - 无测试逻辑：`runSuite` 错误形态分流、`applyReadonlyState` 置灰矩阵、`renderFileList` 过滤、主题持久化语义（persist 参数已修复但无守护）
   - 最低可行方案：引入裸 Node 断言脚本跑纯函数抽取（`escapeHtml`/`filterFiles` 先行），或接受"人工冒烟清单"现状并登记（现状规范 §3.6 允许，但需登记）
2. **TUI 三文件零直测**：
   - `ws_interactive.rs`：`start_session`/`send_text` 的错误路径（非 @websocket/会话冲突）仅经 actions 间接覆盖；重连退避在 ws::session 侧亦无直测
   - `integrations.rs`（V 重放/M mock 开关）：仅 features_test 1 测触达 M；V 的快照缺失/解析失败分支未覆盖
   - `event.rs`：纯数据定义（可接受零测），登记说明即可
3. **`middleware/` 直接单测薄**（6 测）：cookie 生产逻辑零 unwrap ✓，但 0600 权限/锁恢复语义的守护分散在 adversarial_test——建议收敛为 cookie 专属测试模块（可读性，非缺失）

### P2（覆盖缺口，按需顺手补）

4. **`utils/formatter.rs` 零测**：响应格式化器（CLI 输出核心路径），建议补 Compact/Verbose 两形态快照测试
5. **`generator/http.rs` format_entry 零直测**：header 渲染四分支（skip/T2 溯源/凭据模板化/T1 形状）经 e2e 间接覆盖——非 ASCII lossy 新语义建议补直测
6. **`template/strategy.rs` 零直测**：两策略经 CLI 覆盖，策略级单测可防 include_str! 资产漂移
7. **`utils/text.rs`**（截断工具，2 测）：`truncate_utf8_bytes` 的 CJK/省略符交互分支可补
8. **smoke 矩阵缺口**：quick_auth 流（A 键→注入→wire 验证已有 features 测）、搜索/过滤模态组合已有——未触及路径主要是多模态叠加渲染
9. **`run_diagnose` 大写用例**为弱断言（is_object）——schema 键级断言可对齐小写分支

### P3（风格/登记）

10. `metadata.rs:148` 守卫 unwrap → let-else；`report/mod.rs` 尾部 use 已上移；serialization 同名多值覆盖已登记 backlog ✓

---

## 四、走查确认的健康面（抽查通过项）

* **守卫测试红绿抽验 5/5 通过**：cookie_persistence（串行落盘/并行不落盘分档）、sse_stream_resilience（断流 flush/1MB 截断，socket2 RST 实证）、single_request（SKIP 标记 + wiremock 零流量）、adversarial（三 panic 原始复现）、exporter shell（控制字符八进制转义）
* **生产 unwrap 卫生**：cookie.rs 25 个 unwrap 全在 tests；metadata.rs 33 个同；抽查的 1 个生产守卫 unwrap 有前置不变量
* **大小写口径**：`== "json"/"html"` 裸比较 src/ 内清零（唯一残留 http/mod.rs:31 为 diagnose 独立面，已统一 eq_ignore_ascii_case）
* **规范 §2.2 正面**：Phase 1 迁移后 stream.rs 已显式 HashMap；tree/layout/creation/lifecycle/search/retention 均显式 imports
* **规范 §6 正面**：web 五暴露面（host 守卫/save 沙箱/assets 白名单/readonly×3 端点/dev 回环强制）+ mock 127.0.0.1 绑定，全部测试锁定

---

## 五、补齐路线建议

1. **立即可做（半天）**：formatter 快照测试、template 策略直测、generator format_entry 直测（非 ASCII lossy 锁定）、event.rs 零测登记说明
2. **随 Phase 3 后续（一天）**：ws_interactive 错误路径直测（复用 ws_session 测试基建）、integrations V 分支补测、middleware cookie 测试收敛
3. **需决策（立项）**：Web 前端 JS 测试基建（裸 Node 断言 vs 接受人工冒烟现状并登记）——影响后续所有前端功能（Run All/拖拽/编辑器增强）的守护成本
4. **不做**：glob 显式化（有意设计）、metadata 守卫 unwrap 改写（不变量清晰）
