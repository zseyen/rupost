# 04 - 跨场景端到端测试方案与防护网 (Testing Strategy)

- **状态**: Approved
- **归属任务**: `cross-scenario-engine-and-plugins`
- **目标**: 针对“开发环境（富插件）、发布环境（CI 基础）、调试环境（即拷即走无依赖）”建立确定性的端到端测试守护。

---

## 1. 测试金字塔与场景矩阵

```
                ┌────────────────────────────────────────────────────────┐
                │        Layer 4: 跨环境产物消费测试 (Artifact Mining)    │
                │        (调试机 dump.json ──> 开发机富插件深度挖掘)      │
                ├────────────────────────────────────────────────────────┤
                │        Layer 3: 恶劣环境即拷即走测试 (Zero-Dep Portable)│
                │        (在 0 Python/Node 的裸容器内运行基础测试闭环)    │
                ├────────────────────────────────────────────────────────┤
                │        Layer 2: 插件协议与优雅降级集成测试 (Degradation)│
                │        (缺失插件跳过、未授权拦截、超时杀进程熔断)      │
                ├────────────────────────────────────────────────────────┤
                │        Layer 1: 基础单元测试 (Unit & Red-Green)        │
                │        (请求体保真、内置宏向量、JUnit 生成、时延采集)   │
                └────────────────────────────────────────────────────────┘
```

---

## 2. 核心场景专项测试用例设计

### 测试套件 1：调试环境“即拷即走”自包含验证 (Pure Standalone Isolation)
* **目标**：验证在**完全没有 Python、Node、外部可执行文件**的环境中，`rupost` 依然 100% 具备发包、加密加签、断言判定与快照落盘能力。
* **测试用例实现 (`tests/standalone_portable_test.rs`)**：
  ```rust
  #[tokio::test]
  async fn test_standalone_execution_without_external_runtime() {
      // 1. 清空 PATH 环境变量中的插件目录，模拟极简裸机
      let isolated_env = [("PATH", "/usr/bin:/bin")]; // 绝不包含任何 rupost-plugin-*
      
      // 2. 运行包含纯 Rust 内置加签宏的测试用例
      let http_content = r#"
      POST http://127.0.0.1:8080/secure-api
      X-Timestamp: {{$timestamp_ms}}
      X-Sign: {{$hmac_sha256("secret", "{{$timestamp_ms}}:{{$body}}")}}
      Content-Type: application/json

      {"order_id": 999}

      # @assert status == 200
      "#;

      // 3. 执行测试并保存快照
      let outcome = run_isolated_test(http_content, &isolated_env, "/tmp/dump.json").await.unwrap();
      assert!(!outcome.has_failure);
      assert!(std::path::Path::new("/tmp/dump.json").exists());
  }
  ```

---

### 测试套件 2：插件缺失与自愈降级验证 (Graceful Degradation Test)
* **目标**：验证调试环境遇到未安装的插件时，测试流程不崩溃，基础发包与数据记录正常闭环。
* **测试用例实现 (`tests/plugin_degradation_test.rs`)**：
  1. **未声明降级（默认安全拦截）**：
     - 用例包含 `@plugin non_existent_signer`；
     - 未传 `--allow-missing-plugins`；
     - 断言：返回明确的错误提示（包含搜索过的路径清单），进程非零退出。
  2. **声明 `optional=true` 或 `--allow-missing-plugins`（优雅自愈）**：
     - 用例包含 `@plugin non_existent_signer optional=true`；
     - 断言：测试正常发送请求（使用预设的兜底 Header），测试通过；
     - 导出的快照 JSON 中，明确包含 Warning：`"插件 'non_existent_signer' 未安装，已降级跳过"`。

---

### 测试套件 3：跨环境产物深度挖掘验证 (Artifact Mining & Consumption Test)
* **目标**：验证调试环境生成的产物，拷回开发环境后能被插件 100% 消费并挖掘深层信息。
* **测试用例实现 (`tests/artifact_mining_test.rs`)**：
  ```rust
  #[tokio::test]
  async fn test_artifact_can_be_mined_by_offline_plugins() {
      // 1. 模拟在调试环境产出带 504 错误与 Envoy 诊断的 dump.json
      let dump_file = generate_simulated_debug_dump().await;

      // 2. 模拟本地开发环境的插件消费该产物 (反序列化 ExecutionSnapshotFile)
      let content = std::fs::read_to_string(&dump_file).unwrap();
      let snapshot: ExecutionSnapshotFile = serde_json::from_str(&content).unwrap();

      // 3. 验证关键性能指标与现场未丢失
      assert_eq!(snapshot.results[0].status, Some(504));
      let timing = snapshot.results[0].timing_waterfall.as_ref().unwrap();
      assert!(timing.dns_lookup_us > 0);
      assert!(timing.ttfb_us > 4_000_000); // 识别出 4s+ 的 TTFB 瓶颈

      // 4. 验证 AI 归因输入适配器能基于该快照提取 FailureContext
      let ctx = crate::ai::rules::context_from_snapshot_entry(&snapshot.results[0]);
      assert!(ctx.error_message.contains("504"));
  }
  ```

---

### 测试套件 4：请求体字节保真度验证 (Byte-Level Fidelity Test)
* **目标**：验证 JSON 请求体在任何环境下均未被 `serde_json` 偷偷重排或抹除空白。
* **测试用例实现 (`tests/byte_fidelity_test.rs`)**：
  * 使用 WireMock 监听原始 TCP 字节流；
  * 发送带有换行、多级缩进、以及 `100.00` 尾随零的复杂 JSON；
  * 对比 WireMock 收到的裸字节与输入文本，断言 **MD5 校验和 100% 绝对一致**。

---

## 3. 全量自动化执行门禁

在提交任何环境适配与插件代码前，必须在本地运行以下一键回归链：
```bash
# 1. 基础单测与代码规范
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2. 场景化测试套件专项验证
cargo test --test standalone_portable_test
cargo test --test plugin_degradation_test
cargo test --test artifact_mining_test
cargo test --test byte_fidelity_test

# 3. 集成与全量示例回归
bash tests/verify_features.sh
bash examples/run_all.sh
```
