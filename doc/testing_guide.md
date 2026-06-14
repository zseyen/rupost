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
| [`batch_testing_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/batch_testing_test.rs) | 拓扑 DAG、并行调度、沙箱防护及状态单向克隆 | 验证：Kahn 算法拓扑排序、有向依赖环检测、并行模式下的并发限制、Fail-Fast 流程中断，以及有依赖节点间 `State Cloning`。<br>包含对路径穿越越权拦截 `test_dependency_resolver_sandbox_jail` 的测试。 |
| [`diagnose_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/diagnose_integration_test.rs)<br>[`timing_diagnostics_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/timing_diagnostics_test.rs) | 连通性分析与 X.509 解析 | 验证：时延时序拆分的计算合理性、证书 Validity 的剩余天数计算、临期/过期判定。 |
| [`mock_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/mock_integration_test.rs) | Trie 树匹配与变体评估 | 验证：路径参数捕获（`:id`）、通配符匹配、Header/Query/Body JSONPath 变体匹配及兜底逻辑。 |

---

## 2. 手工功能验证大纲 (E2E Manual Testing Checklist)

为了帮助您手动检验 RuPost 的各项功能是否完全正常工作，您可以执行以下手工验证步骤：

### 2.1 验证 1：模板配置快速初始化 (rupost init)
1.  **准备环境**：选择一个不存在 `rupost.toml` 的空临时文件夹。
2.  **执行命令**：
    ```bash
    cargo run -q -- init
    ```
    *   **预期输出**：控制台输出绿色高亮 `[✓] 成功在当前目录初始化默认 rupost.toml 模板！`。
    *   **结果校验**：检查当前目录下已生成标准的 `rupost.toml` 文件。
3.  **防覆盖校验**：修改生成的 `rupost.toml`，在末尾手动追加一行 `# my-custom-config`。再次执行 `cargo run -q -- init`。
    *   **预期输出**：控制台输出黄色高亮 `[!] rupost.toml 已经存在于当前目录，跳过初始化。`。
    *   **结果校验**：检查文件内容，先前追加的自定义内容依旧完整保留，未被篡改。

### 2.2 验证 2：多层级变量级联覆盖 (Cascading Priority)
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
3.  **手工执行指令与校验**：
    *   **场景 A：使用本地 `.env` 默认值**
        ```bash
        cargo run -q -- test check_var.http
        ```
        *   **期望结果**：响应 headers 中 `X-Test-Var` 渲染输出为 `"local-env-val"`。
    *   **场景 B：使用进程系统环境变量覆盖**
        ```bash
        test_key="system-env-val" cargo run -q -- test check_var.http
        ```
        *   **期望结果**：响应中的 `X-Test-Var` 渲染输出为 `"system-env-val"`。同时，系统上其他无关的环境变量（如 `PATH`）不会被带入用例上下文（防范命名污染）。
    *   **场景 C：使用 CLI 命令行参数覆盖**
        ```bash
        cargo run -q -- test check_var.http --var test_key=cli-override
        ```
        *   **期望结果**：这是最高优先级，响应中的 `X-Test-Var` 必定输出为 `"cli-override"`。

### 2.3 验证 3：全局共享变量 (global.) 与系统环境变量直接映射 (env.)
1.  **启动本地 Mock 支持**：编写 Mock 规则 `mock_global.json` 并在端口 `9000` 启动：
    ```json
    [
      { "method": "POST", "path": "/api/auth", "variants": [{ "condition": null, "status": 200, "headers": {}, "response_body": "{\"token\":\"shared-auth-token-888\"}" }] },
      {
        "method": "GET",
        "path": "/api/user",
        "variants": [
          { "condition": { "source": "Header", "key": "Authorization", "operator": "Equals", "expected_value": "Bearer shared-auth-token-888" }, "status": 200, "headers": {}, "response_body": "{\"status\":\"ok\"}" }
        ]
      }
    ]
    ```
    ```bash
    cargo run -q -- mock mock_global.json --port 9000
    ```
2.  **编写 DAG 依赖测试文件**：
    *   `01_login.http`：
        ```http
        POST http://127.0.0.1:9000/api/auth
        
        @capture global.token from body.token
        ```
    *   `02_profile.http`：
        ```http
        ### @depends-on 01_login.http
        GET http://127.0.0.1:9000/api/user
        Authorization: Bearer {{global.token}}
        X-CurrentUser: {{env.USER}}
        ```
3.  **运行并行并发测试**：
    ```bash
    cargo run -q -- test 02_profile.http --mode parallel
    ```
    *   **期望结果**：控制台显示 `01_login.http` 与 `02_profile.http` 极速通过测试。
    *   **结果分析**：
        1.  `global.token` 作为跨线程的原子读写共享变量，被 `01_login.http` 成功修改并跨分支单向克隆（State Cloning）传递给了子节点 `02_profile.http`。
        2.  `02_profile.http` 发送请求时 `X-CurrentUser` 成功被系统解析为当前计算机登录用户名（利用 `env.USER` 动态映射机制）。

### 2.4 验证 4：跨文件拓扑依赖 (DAG) 与安全沙箱加载防护
1.  **依赖环路自检**：在 `01_login.http` 的头部人为加上 `### @depends-on 02_profile.http`，使依赖链变成 `01_login -> 02_profile -> 01_login`，运行测试。
    *   **预期输出**：RuPost 应当检测出循环依赖并抛出错误终止测试。
2.  **安全沙箱穿越校验 (Path Traversal)**：
    *   尝试在 `02_profile.http` 中引用沙箱工作目录之外的绝对路径或多级相对跳出路径，如：
        ```http
        ### @depends-on ../../../../../etc/passwd
        ```
    *   执行测试。
    *   **预期输出**：终端应当抛出类似错误并立即中断：
        `Error: Other("安全违规：依赖文件不在沙箱目录内，拒绝加载！文件: /private/etc/passwd")`
3.  **后缀后缀名校验**：
    *   尝试依赖一个处于工作区内但是为非 HTTP/MD 类型的文件：
        ```http
        ### @depends-on Cargo.toml
        ```
    *   执行测试。
    *   **预期输出**：终端报错 `不受支持的测试文件类型，拒绝加载！文件: .../Cargo.toml`。

### 2.5 验证 5：网络诊断工具 (Diagnose)
1.  **执行命令**：
    ```bash
    cargo run -q -- diagnose https://www.google.com
    ```
2.  **结果观察指标**：
    *   时延瀑布条形图是否完整显示了 DNS Lookup、TCP Connect、TLS Handshake 和 HTTP TTFB 的各自耗时占比。
    *   X.509 证书的 Subject (SAN)、Issuer 以及剩余有效天数是否成功解析并高亮：
        *   若证书已到期，终端显示红色 `[EXPIRED]`。
        *   若到期时间 <= 30 天，终端显示黄色警告 `[WARNING]`。
        *   其余到期时间显示绿色 `[VALID]`。

### 2.6 验证 6：Mock 服务端与分支条件匹配
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

## 3. 一键式全特性集成验证脚本 (verify_features.sh)

在测试和开发过程中，如果想快速判定所有的特性是否有 regression（功能退化），我们提供了一键化的端到端测试与特性演示脚本：

### 3.1 运行方式
在项目根目录下，直接执行：
```bash
chmod +x tests/verify_features.sh
./tests/verify_features.sh
```

### 3.2 脚本核心验证逻辑
为了摆脱公共网络不稳定的影响，该集成脚本将整体测试转化为**纯本地自闭环测试**：
1.  自动在当前工作区编译出最新的 release 二进制。
2.  在当前目录下的合法测试路径 `./tests_temp_dir/` 创建临时测试沙箱。
3.  验证 `rupost init` 命令能否成功在空目录下初始化 `rupost.toml`，并测试防覆盖校验是否生效。
4.  在后台拉起 Mock 服务器。
5.  触发 Mock 路径参数捕获（`:id` 动态渲染）与 `X-Role: Admin` / `Guest` 变体多路条件匹配。
6.  对本地处于活动状态的 Mock 服务端口执行 `rupost diagnose` 网络诊断，检验条形时延图与状态分析。
7.  在该 Mock 服务上跑包含 Fork-Join DAG 拓扑关系的并行测试。验证前置步骤获取的 `global.token` 变量能够被单向克隆（State Cloning）传递到子节点的上下文，并能级联覆盖本地的 `.env` 配置变量。
8.  一键清理全部后台进程与临时测试工作区。

---

## 4. 测试套件执行与代码覆盖率

### 4.1 运行所有 Rust 原生测试
在项目根目录下，直接运行 cargo 测试指令：
```bash
cargo test
```

### 4.2 运行代码覆盖率统计
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
