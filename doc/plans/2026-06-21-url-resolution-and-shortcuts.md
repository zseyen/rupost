# URL 智能拼接与快捷缩写设计规范 (URL Resolution & Shortcuts Spec)

## 1. 背景与目标
在 API 开发与测试中，URL 的书写频率极高。当前 Rupost 要求所有的相对路径必须显式配置全局 `base_url`（如在 `.env` 中）或局部 `base_path`，对于“随手测试”或“本地微服务调试”不够便捷。

本设计旨在借鉴 **HTTPie** 的优秀设计，并结合 Rupost 的 **文件驱动特性**，制定一套适用于 **CLI 场景** 与 **文件文档场景** 的 URL 智能拼接与简写规范。

---

## 2. 核心场景划分与规则定义

我们将使用场景严格划分为 **命令行临时运行（CLI）** 和 **测试文件运行（Document）** 两类，以在“敏捷性”与“工程稳定性”之间取得完美平衡。

### 场景 A：CLI 临时请求场景 (CLI Temporary Execution)
**定位**：用于快速、单次的命令行调试，类似于 `curl` 或 `httpie`。

1. **智能协议补全 (Implicit Scheme)**:
   - 若输入的 URL **不包含** 协议头（`://`），且满足以下“独立主机”特征之一，则判定其为完整地址，**自动补全默认协议 `http://`**（而非判定为相对路径）：
     - 规则：首段路径（第一个 `/` 之前）包含 `.`（如域名 `api.github.com/users`）或包含 `:` 且后接数字（如 `localhost:3000/users`）。
     - 示例：`rupost run api.github.com/users` $\rightarrow$ 发起请求至 `http://api.github.com/users`。
2. **冒号本地快捷键 (Localhost Shorthand)**:
   - 允许以 `:` 开头指定本地服务，自动拼装为 `localhost`：
     - `:3000/users` $\rightarrow$ `http://localhost:3000/users`
     - `:/health` $\rightarrow$ `http://localhost/health`
     - `:` $\rightarrow$ `http://localhost`
3. **协议头控制**:
   - 命令行支持 `--default-scheme=https` 参数，用以将上述补全规则的默认协议改为 `https://`。

---

### 场景 B：文件与文档场景 (HTTP & Markdown Document Execution)
**定位**：用于编写、沉淀与团队共享的 API 测试套件，重在**环境无关性**、**可复用性**与**健壮性**。

1. **层级自动拼装规范 (Hierarchical Auto-Join)**:
   - 必须保持绝对的环境解耦。当请求行写为纯相对路径（如 `GET /users`）时，**不进行盲目的 localhost 默认退化**。
   - 必须严格遵循以下拼装公式：
     $$\text{Final URL} = \text{[全局 base\_url]} + \text{[文件级 base\_path]} + \text{[请求 URL]}$$
   - 若最终未配置任何基准地址，依然抛出 `BaseUrlNotConfigured`，以避免测试用例在不同环境下因隐式默认值导致不可预测的行为。
2. **文档内局部本地快捷键 (Localhost Shorthand in Docs)**:
   - 在 `.http` 或 `.md` 文件的请求行中，**允许显式使用冒号快捷方式**。
   - 示例：
     ```http
     ### 本地 Auth 服务快速联调
     POST :8081/auth/login
     Content-Type: application/json
     
     {
       "username": "admin"
     }
     ```
   - **拼装规则**：直接无条件将首部 `:` 替换为 `http://localhost` 并拼接后续内容。这避免了本地微服务调试时，需要在变量中定义十几个不同端口的 `auth_base_url`、`user_base_url` 的繁琐。

---

## 3. 技术设计与数据流转 (Technical Design)

### 3.1 路径判定状态机

在 `src/runner/executor.rs` 的 `execute_one` 中，URL 解析逻辑重构如下：

```mermaid
graph TD
    Start[获取原始 URL] --> CheckProto{是否包含 ://}
    
    CheckProto -->|是| Absolute[直接使用绝对路径]
    
    CheckProto -->|否| CheckShorthand{是否以 : 开头?}
    
    CheckShorthand -->|是| LocalhostResolve[将首部 : 替换为 http://localhost]
    
    CheckShorthand -->|否| CheckHostFeature{首段路径是否包含 . 或 :?}
    
    CheckHostFeature -->|是| ImplicitScheme[补全默认协议 http://]
    
    CheckHostFeature -->|否| RelativeResolve[作为相对路径处理]
    RelativeResolve --> HierarchicalJoin[执行 [全局 base_url] + [文件级 base_path] + url 拼接]
```

### 3.2 伪代码实现

```rust
fn resolve_final_url(
    raw_url: &str, 
    context: &VariableContext, 
    default_scheme: &str
) -> Result<String, RupostError> {
    // 1. 处理绝对 URL
    if raw_url.contains("://") || raw_url.starts_with("http://") || raw_url.starts_with("https://") {
        return Ok(raw_url.to_string());
    }

    // 2. 处理冒号 localhost 缩写
    if raw_url.starts_with(':') {
        let suffix = &raw_url[1..];
        let base = format!("{}://localhost", default_scheme);
        return Ok(join_paths(&[&base, suffix]));
    }

    // 3. 处理独立主机判断（智能协议补全）
    let first_segment = raw_url.split('/').next().unwrap_or("");
    if first_segment.contains('.') || first_segment.contains(':') {
        return Ok(format!("{}://{}", default_scheme, raw_url));
    }

    // 4. 作为相对路径，进行分层拼接
    let global_base = get_global_base(context);
    let file_base = context.get("__file_base_path");

    match (global_base, file_base) {
        (Some(g), Some(f)) => {
            if f.contains("://") {
                Ok(join_paths(&[&f, raw_url]))
            } else {
                Ok(join_paths(&[&g, &f, raw_url]))
            }
        }
        (Some(g), None) => Ok(join_paths(&[&g, raw_url])),
        (None, Some(f)) => Ok(join_paths(&[&f, raw_url])),
        (None, None) => Err(RupostError::BaseUrlNotConfigured),
    }
}
```

---

## 4. 验证计划 (Verification Plan)

### 4.1 单元测试设计
在 `src/runner/executor.rs` 的 `tests` 模块中增加：
* `test_cli_implicit_scheme_completion`：验证 `api.github.com/users` 自动补全为 `http://api.github.com/users`。
* `test_localhost_port_shorthand`：验证 `:3000/health` 自动拼接为 `http://localhost:3000/health`。
* `test_document_hierarchical_fallback`：验证在没有任何配置时，纯相对路径 `/users` 抛出 `BaseUrlNotConfigured`，而 `:3000/users` 能够成功执行。

### 4.2 E2E 测试验证
在 `tests/end_to_end_test.rs` 中：
* 编写模拟 CLI 请求流程，确保参数 `--default-scheme=https` 能够正确改变补全协议。
