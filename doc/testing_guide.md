# RuPost 测试与功能验证指引 (Testing & Verification Guide)

本文档面向测试工程师与核心开发者，详细梳理了 RuPost 的测试架构矩阵、手工功能验证步骤（E2E Manual Checklist），以及如何执行代码测试与覆盖率分析。

---

## 1. 自动化测试架构矩阵

RuPost 维持了极高的测试覆盖质量，所有的核心业务逻辑和网络适配器均有完备的自动化集成测试。以下是主要的测试用例分布：

| 测试用例文件 | 覆盖的特性与核心功能 | 验证指标 |
| :--- | :--- | :--- |
| [`user_agent_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/user_agent_test.rs) | User-Agent 级联覆盖 | 验证：默认 UA -> 变量配置覆盖 -> 用例 Header 最高优先级覆盖的级联链路。 |
| [`cookie_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/cookie_test.rs)<br>[`cookie_isolation_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/cookie_isolation_test.rs) | Cookie 管理与并行隔离 | 验证：Cookie 自动保存与提取，多并行分支测试时的 Cookie 物理隔离。 |
| [`variable_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/variable_integration_test.rs) | 四层级变量覆盖逻辑 | 验证：CLI、环境变量、本地 `.env` 及 `rupost.toml` 的级联覆盖与优先级。 |
| [`batch_testing_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/batch_testing_test.rs) | 拓扑 DAG、并行调度及状态单向克隆 | 验证：Kahn 算法拓扑排序、有向依赖环检测、并行模式下的并发限制、Fail-Fast 流程中断，以及有依赖节点间 `State Cloning`。 |
| [`diagnose_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/diagnose_integration_test.rs)<br>[`timing_diagnostics_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/timing_diagnostics_test.rs) | 连通性分析与 X.509 解析 | 验证：时延时序拆分的计算合理性、证书 Validity 的剩余天数计算、临期/过期判定。 |
| [`mock_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/mock_integration_test.rs) | Trie 树匹配与变体评估 | 验证：路径参数捕获（`:id`）、通配符匹配、Header/Query/Body JSONPath 变体匹配及兜底逻辑。 |

---

## 2. 手工功能验证大纲 (E2E Manual Testing Checklist)

为了帮助您手动检验 RuPost 的各项功能是否完全正常工作，您可以执行以下手工验证步骤：

### 2.1 验证 1：变量级联覆盖
1.  **准备环境**：在项目根目录下创建一个临时 `.env` 文件：
    ```env
    base_url = "https://httpbin.org"
    test_key = "local-env-val"
    ```
2.  **准备用例**：创建临时文件 `check_var.http`：
    ```http
    GET {{base_url}}/headers
    X-Test-Var: {{test_key}}
    ```
3.  **手工执行指令**：
    ```bash
    # 场景 A: 使用 .env 默认值 (预期 X-Test-Var 为 "local-env-val")
    cargo run -q -- test check_var.http

    # 场景 B: 使用进程环境变量覆盖 (预期 X-Test-Var 为 "system-env-val")
    test_key="system-env-val" cargo run -q -- test check_var.http

    # 场景 C: 使用 CLI 参数覆盖 (最高优先级，预期 X-Test-Var 为 "cli-override")
    cargo run -q -- test check_var.http --var test_key=cli-override
    ```

### 2.2 验证 2：跨文件拓扑依赖 (DAG) 与串/并行运行
1.  **准备用例**：在临时目录内创建两个相互依赖的文件：
    *   `01_auth.http`：
        ```http
        POST https://httpbin.org/post
        Content-Type: application/json
        
        { "token": "super-secret-token" }
        
        @capture auth_token = body.json.token
        ```
    *   `02_user.http`：
        ```http
        ### @depends-on 01_auth.http
        GET https://httpbin.org/headers
        Authorization: Bearer {{auth_token}}
        ```
2.  **手工执行指令**：
    ```bash
    # 场景 A: 顺序模式执行 (验证 01_auth.http 先于 02_user.http 运行，且 02_user.http 获取了 Token)
    cargo run -q -- test 02_user.http
    
    # 场景 B: 并行模式执行 (验证并发克隆传递 Context)
    cargo run -q -- test 02_user.http --mode parallel
    
    # 场景 C: 循环依赖测试 (预期报错循环依赖)
    # 在 01_auth.http 头部人为加入 ### @depends-on 02_user.http，再执行:
    cargo run -q -- test 02_user.http
    ```

### 2.3 验证 3：网络诊断工具 (Diagnose)
1.  **手工执行指令**：
    ```bash
    cargo run -q -- diagnose https://www.google.com
    ```
2.  **结果观察指标**：
    *   检查终端中是否输出了 HSL 彩色条形瀑布图。
    *   检查 `Resolved IPs` 列表是否正确打印。
    *   证书信息栏是否成功提取出 `Subject` 和 `Issuer`。
    *   证书到期天数与 `Status` 标签是否高亮呈现（根据过期天数显示 `[VALID]`、`[WARNING]` 或 `[EXPIRED]`）。

### 2.4 验证 4：Mock 服务端与分支条件匹配
1.  **准备 Mock 定义**：创建配置文件 `mock_test.json`：
    ```json
    [
      {
        "method": "POST",
        "path": "/api/users/:id",
        "variants": [
          {
            "condition": {
              "source": "Header",
              "key": "X-Role",
              "operator": "Equals",
              "expected_value": "Admin"
            },
            "status": 200,
            "headers": { "Content-Type": "application/json" },
            "response_body": "{\"status\":\"success\",\"role\":\"admin\",\"user_id\":\"{{id}}\"}"
          },
          {
            "condition": null,
            "status": 403,
            "headers": { "Content-Type": "application/json" },
            "response_body": "{\"error\":\"Access Denied\"}"
          }
        ]
      }
    ]
    ```
2.  **拉起服务**：
    ```bash
    cargo run -q -- mock mock_test.json --port 9000
    ```
3.  **发送匹配验证请求**：打开另一个终端，进行如下验证：
    ```bash
    # 场景 A: 满足 Variant 条件 (预期命中 Admin 响应，且 user_id 渲染为 99)
    curl -i -X POST http://localhost:9000/api/users/99 -H "X-Role: Admin"

    # 场景 B: 不满足 Variant 条件 (预期进入兜底返回 403)
    curl -i -X POST http://localhost:9000/api/users/99 -H "X-Role: Guest"
    ```

---

## 3. 测试套件执行与代码覆盖率

### 3.1 运行所有 Rust 原生测试
在项目根目录下，直接运行 cargo 测试指令：
```bash
cargo test
```

### 3.2 运行代码覆盖率统计
我们推荐使用 `cargo-tarpaulin` 工具进行覆盖率分析。
1.  **安装工具**（若未安装）：
    ```bash
    cargo install cargo-tarpaulin
    ```
2.  **生成覆盖率报告**：
    ```bash
    cargo tarpaulin --out Html --output-dir target/tarpaulin/
    ```
    运行结束后，您可以在浏览器中打开 `target/tarpaulin/tarpaulin-report.html` 查看每一行代码的覆盖状态。
