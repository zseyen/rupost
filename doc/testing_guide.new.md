# RuPost 测试与功能验证指引 (Testing & Verification Guide)

本文档面向测试工程师与核心开发者，详细梳理了 RuPost 的测试架构矩阵、基于 `examples/` 示例的 E2E 手工验证步骤，以及如何执行代码测试与覆盖率分析。

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

## 2. 基于内置示例的手工功能验证大纲 (E2E Manual Testing Checklist)

为了能够零门槛、免碎文件干扰地手动验证 RuPost 的核心能力，你可以直接使用 codebase 中内置在 `examples/` 下的各类 HTTP/Markdown 演示用例。

### 2.1 验证 1：模板配置快速初始化 (rupost init)
1.  **准备环境**：新建一个没有任何配置的空文件夹，如 `mkdir /tmp/rupost_test && cd /tmp/rupost_test`。
2.  **执行命令**：
    ```bash
    cargo run --bin rupost -- init
    ```
    *   **预期输出**：控制台输出绿色高亮 `[✓] 成功在当前目录初始化默认 rupost.toml 模板！`。
    *   **结果校验**：检查当前目录下已生成标准的 `rupost.toml` 配置文件，且包含开发、测试、生产等多套配置。
3.  **防覆盖校验**：修改生成的 `rupost.toml`，在末尾手动追加一行 `# my-custom-config`。再次执行 `cargo run --bin rupost -- init`  。
    *   **预期输出**：控制台输出黄色高亮 `[!] rupost.toml 已经存在于当前目录，跳过初始化。`。
    *   **结果校验**：检查文件内容，先前追加的自定义内容依旧完整保留，未被篡改。

### 2.2 验证 2：多层级变量级联覆盖优先权
1.  **准备环境**：我们直接结合 `examples/auth-flow.http` 以及 `examples/rupost.toml` 演示。
2.  **运行基本场景（使用 `rupost.toml` 中的默认配置）**：
    ```bash
    cargo run --bin rupost -- test examples/auth-flow.http --env dev
    ```
    *   **执行说明**：会使用 `examples/rupost.toml` 中 `dev` 段定义的 `base_url = "https://httpbingo.org"` 变量。
    *   **检验结果**：用例应当跑通（因为 httpbingo.org 会返回正常响应）。
3.  **运行命令行变量覆盖场景（最高优先级）**：
    ```bash
    cargo run --bin rupost -- test examples/auth-flow.http --env dev --var base_url=https://httpbin.org
    ```
    *   **执行说明**：我们将 `base_url` 在命令行中强制覆盖为 `https://httpbin.org`。
    *   **检验结果**：命令行输出中，原本指向 `httpbingo.org` 的请求全部发向了 `httpbin.org`。证明命令行变量覆盖已成功发挥最高优先级。
4.  **运行环境防命名空间污染校验**：
    ```bash
    # 注入一个与系统环境同名的不相干环境变量
    api_version="v999" cargo run --bin rupost -- test examples/auth-flow.http --env dev
    ```
    *   **结果校验**：请求依然成功向 `v1`（从 `rupost.toml` 获取）发送，而不是被外部的 `api_version=v999` 篡改，证明沙箱变量解析器对系统环境变量进行了严格命名过滤隔离。

### 2.3 验证 3：全局变量捕获 (global.) 与状态克隆 (State Cloning)
1.  **运行示例用例集**：直接跑 `examples/batch/` 目录：
    ```bash
    cargo run --bin rupost -- test examples/batch/ --env dev
    ```
    *   **观察依赖解析与执行顺序**：
        1.  由于 `02_get_profile.http` 头部声明了 `### @depends-on 01_login.http`，DAG 调度器在并行模式下会先执行 `01_login.http`。
        2.  `01_login.http` 向 `https://httpbingo.org/post` 模拟登录并携带密码 `secret_token_abc123`，并利用 `@capture global.token from body.json.password` 将其写入 `global.token` 变量。
        3.  `02_get_profile.http` 拿到 `01_login.http` 跑完之后的克隆上下文状态，成功将其渲染进 `Authorization: Bearer {{global.token}}`。
        4.  控制台最后通过两个断言，全部显示 `passed`。

### 2.4 验证 4：复杂有向依赖网 (DAG) 调度与依赖环检测
1.  **并行执行复杂拓扑用例**：
    ```bash
    cargo run --bin rupost -- test examples/batch_complex/ --env dev --mode parallel
    ```
    *   **观察拓扑控制**：
        *   系统会根据依赖图（`01_auth.http` -> `02_init_data.http` -> 并行 `03_get_category.http` / `03_publish_post_1.http` / `03_publish_post_2.http` -> `04_cleanup.http`）进行精准的多线程并行发包调度。
2.  **验证循环依赖检测**：
    *   打开 `examples/batch_complex/01_auth.http` 文件，在头部人为追加一行 `### @depends-on 04_cleanup.http`（使其与 04_cleanup 构成强连通循环依赖圈）。
    *   再次执行 `cargo run --bin rupost -- test examples/batch_complex/`。
    *   **预期输出**：终端抛出清晰的错误：`Error: Dependency loop detected`，并直接中断，避免线程死锁。
    *   **恢复文件**：撤销对 `01_auth.http` 的修改。

### 2.5 验证 5：网络诊断 waterfall 瀑布时延拆分与过期证书告警
1.  **运行高延迟诊断**：
    在 `examples/sprint1_diagnostics_demo.http` 中有一条高延迟（1秒等待）的请求。
    ```bash
    cargo run --bin rupost -- test examples/sprint1_diagnostics_demo.http --env dev --debug
    ```
    *   **观察输出**：输出的时序条（Latency Breakdown）会非常直观地展现出高延迟来源于 `Server Wait (TTFB)`（服务器内部时延占大头），而不是网络 TCP 握手。
2.  **执行 HTTPS 证书生命周期校验**：
    ```bash
    cargo run --bin rupost -- diagnose https://expired.badssl.com
    ```
    *   **观察输出**：X.509 证书段中，对于这块已过期的证书，其 Validity 会以醒目的红色输出 `[EXPIRED]`。

### 2.6 验证 6：Mock 服务端模糊路由与 Headers 变体匹配
我们将使用 `benches/load_test/mock_perf_config.json` 规则文件来验证 Mock 功能：
1.  **启动 Mock 服务**：
    ```bash
    cargo run --bin rupost -- mock benches/load_test/mock_perf_config.json --port 9000
    ```
    *   此时 Mock 服务会拉起在本地端口 9000。
2.  **验证 Trie 路径参数模糊匹配 & 变体匹配成功情况**：
    打开另一个终端发送：
    ```bash
    curl -i -X GET http://127.0.0.1:9000/api/users/12345 -H "X-Role: Admin"
    ```
    *   **期望返回**：`HTTP/1.1 200 OK`，且 Body 为 `{"status":"success","role":"Admin","user_id":"12345"}`（验证了模糊匹配的路径参数成功捕获并渲染回 Body，且 Header 匹配变体分支正确生效）。
3.  **验证 Mock 兜底分支拦截**：
    ```bash
    curl -i -X GET http://127.0.0.1:9000/api/users/12345 -H "X-Role: Guest"
    ```
    *   **期望返回**：`HTTP/1.1 403 Forbidden`，且 Body 为 `{"error":"Access Denied"}`（由于 role 不是 Admin，自动滑入 condition 为 null 的兜底分支）。

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
