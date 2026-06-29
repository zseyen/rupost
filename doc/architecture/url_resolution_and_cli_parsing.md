# URL 智能拼接与 CLI 语法解析组件架构设计

本文件详述了 RuPost 针对命令行界面（CLI）多语法解析以及用例执行中核心 URL 补全逻辑的子组件架构划分与设计细节。

---

## 1. 架构演进背景

在旧版本中，RuPost 命令行执行和测试用例运行涉及的字符串解析逻辑处于高耦合状态：
1. **CLI 语法解析器耦合**：`CliRunner` 既负责 Clap 配置加载、主任务分发，又需要负责解析具体的 `curl` / `httpie` 参数细节，导致 `src/cli.rs` 体积庞大，难以扩展其它命令行输入。
2. **URL Resolution 耦合**：URL 智能拼装与冒号本地快捷键替换算法直接写在 `src/runner/executor.rs` 的私有辅助函数中，与 `TestExecutor` 的运行态状态绑定。测试 URL Resolution 必须构造复杂的 `TestExecutor` 或 `VariableContext`，降低了可测试性。

为达成 **Clean Architecture** 规范，我们执行了组件化重构，抽取了两个纯粹的、无状态的底层服务组件。

---

## 2. 组件架构设计

重构后的模块分层与调用关系如下：

```mermaid
graph TD
    subgraph 接口适配层 (CLI Layer)
        Cli[src/cli.rs - CliRunner] -->|调用无状态解析| Parser[src/cli/parser.rs]
        Parser -->|构建出标准请求| ParsedRequest[ParsedRequest]
    end

    subgraph 核心引擎层 (Runner Layer)
        Executor[src/runner/executor.rs - TestExecutor] -->|解析最终 URL| UrlResolution[src/runner/url.rs]
        UrlResolution -->|拼接出最终绝对地址| Executor
    end

    ParsedRequest --> Executor
```

---

## 3. 技术实现细节

### A. CLI 语法解析组件 (`src/cli/parser.rs`)
该组件承担了 `httpie` 风格与 `curl` 风格命令行输入转换至核心 AST（`ParsedRequest`）的全部细节职责。

#### 1) httpie 严格 JSON (`:=`) 解析
*   **规则**：当输入中包含 `key:=value` 时，代表该字段必须是合法的 JSON 数据类型（如数字、布尔值、对象等）。
*   **设计**：严格调用 `serde_json::from_str` 进行转换。如果转换失败，直接抛出 `RupostError::ParseError`，防止拼写错误（如 `active:=tru`）被默默退化成普通字符串发送至后端。

#### 2) curl 选项防污染防御机制
*   **规则**：当遇到未实现的带参 curl 选项时（例如 `-u`、`-o` 等），不仅打印 `tracing::warn!` 警示信息，还要主动调用迭代器 `args_iter.next()` 消费并跳过它的参数值。
*   **设计**：此机制彻底杜绝了未实现的参数值（如 `myuser`）在随后的遍历中被错判为位置参数（即 URL），保证了 URL 解析的绝对健壮。

---

### B. URL 解析与拼接组件 (`src/runner/url.rs`)
该领域服务负责接收用例声明的 `raw_url` 并根据上下文与调用源头智能拼装出合法的绝对 HTTP/WS 协议 URL。

```
                       ┌─────────────────┐
                       │  Input raw_url  │
                       └────────┬────────┘
                                │
                      [Is absolute URL?]
                                │
                 ┌──────────────┴──────────────┐
                Yes                            No
                 │                             │
        ┌────────┴────────┐            [Starts with :]
        │ Keep unchanged │             ┌───────┴───────┐
        └─────────────────┘            Yes             No
                                        │              │
                           ┌────────────┴───┐    [Is CLI Mode?]
                           │ Localhost Sfx  │     ┌────┴────┐
                           └────────────────┘    Yes        No
                                                  │         │
                                   ┌──────────────┴───┐  [Layered join]
                                   │ Indep. Host Det? │  └──────────┘
                                   └──────────────────┘
```

#### 1) CLI 独立主机智能协议补全
在 CLI 直接执行的请求下，支持探测第一路径分段：
- 若包含 `.`（例如 `api.github.com/users`）或包含 `:` 加数字端口（例如 `localhost:3000/users`），则自动视其为独立主机并根据默认协议头自动补全为 `http://api.github.com/users`。

#### 2) 分层相对路径拼接公式
在文档（Document）执行环境下，对于相对路径按照以下公式顺序进行有向级联：
$$\text{Final URL} = \text{Global base\_url} \oplus \text{File-level base\_path} \oplus \text{Relative path}$$
若最终无法得出绝对协议路径且未配置 base_url，则抛出 `BaseUrlNotConfigured` 错误以确保用例的执行安全性。

---

## 4. 架构收益总结

1. **测试彻底解耦**：
   - CLI 参数解析与 URL Resolution 原有的大批测试用例已同步迁移至对应子组件底部，成为纯粹的纯函数单元测试，运行速度更快且无需复杂状态桩。
2. **遵守开闭原则 (OCP)**：
   - CLI 层若需支持更多客户端工具协议，只需改动 `parser.rs`。
   - 核心执行器 `executor.rs` 的行数大幅缩减，更易维护。
