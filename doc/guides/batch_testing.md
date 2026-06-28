# 批量测试与目录递归执行 (Batch & Directory Testing)

RuPost 支持对整个目录或多个文件进行批量测试。内置了依赖图解析与高效的执行模式。

---

## 1. 递归目录与隐藏路径过滤

当传入一个或多个文件夹时，RuPost 会自动递归扫描并收集所有的 `.http` 与 `.md` 文件。同时为了提高扫描效率，会自动过滤并跳过以下无关或隐藏路径：
* `.git/`
* `.rupost/`
* `target/`
* `node_modules/`

示例：
```bash
# 递归运行整个 examples 文件夹
rupost test examples/
```

---

## 2. 声明式跨文件依赖 (`@depends-on`)

支持在 `.http` 或 `.md` 文件的头部通过 `### @depends-on <filename>` 属性声明前置依赖关系。

例如在 `profile.http` 中：
```http
### @depends-on login.http
GET {{base_url}}/profile
Authorization: Bearer {{my_token}}
```

### 拓扑排序算法 (DAG)
RuPost 会自动构建有向无环图（DAG），并利用 **Kahn 拓扑排序算法** 计算并编排正确的用例文件执行顺序。如果测试用例文件之间检测到循环依赖（Cycle Dependency），RuPost 将打印冲突环路信息并报错退出。

---

## 3. 双执行模式选择

### A. 顺序模式 (Sequential) —— 默认
按拓扑排序后的链式顺序依次串行执行。用例与文件之间**共享变量上下文与 Cookie 会话**。适用于有强状态交互的级联用例链。

### B. 并行模式 (Parallel)
通过 `--mode parallel` 开启，支持通过 `--concurrency <N>` 设置最大的并发限制数。
```bash
rupost t examples/batch/ --mode parallel --concurrency 4
```

* **状态单向克隆 (State Cloning)**：
  在并行运行时，如果两个用例之间存在 DAG 依赖关系，RuPost 会从前置父节点向子节点克隆并合并 Context 变量增量与序列化后的 Cookie。这彻底消除了并行运行下的“数据孤岛”问题，确保鉴权 Token 与 Cookie 能顺利级联；而对于没有依赖关系的分支，则依然维持安全的数据隔离，防止发生并发状态竞争。

---

## 4. 安全沙箱加载防护 (Sandbox Scope Jail)

在递归自动补全加载声明的依赖文件时，RuPost 设立了严格的物理沙箱边界：
* 依赖的绝对物理路径必须位于当前工作目录（CWD）或用户指定的执行目录下。
* 严禁加载任何非沙箱范围的文件或非支持测试扩展名（`.http`/`.md`）的文件。
* 这从底层切断了路径穿越（Path Traversal）安全漏洞，防止恶意脚本读取敏感系统文件。

---

## 5. 高级输出控制

### 早期中断 (`--fail-fast`)
在批量模式下，一旦遇到任何一个用例文件执行失败，立刻强行终止后续所有测试，便于集成构建时快速止损。

### 结构化 JSON 报告 (`--report json`)
支持输出标准化 JSON 结果：
```bash
rupost test examples/ --report json > report.json
```
完美兼容 CI/CD 流程中的自动化断言与数据持久化。
