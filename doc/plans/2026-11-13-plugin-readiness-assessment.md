# 插件承载 OpenAPI 导入 / k6 导出的基础条件评估

**日期：** 2026-11-13
**评估问题：** [Phase 0/1 缺口修复 + Phase 2/3 MVP 执行计划](2026-11-13-phase01-remediation-plan.md) 中的 Phase 3 两项（`import openapi`、`export k6`），能否改由**其他人以 plugin 形式**实现？当前插件基础条件是否满足？
**结论先行：** **命令插件形态今天即可承载"独立转换工具"类实现（OpenAPI 导入可行）；但两项功能以 `import`/`export` 子命令的一等公民形态被插件实现 = 基础条件不满足**（管线无扩展点）。建议维持计划不变（核心内置），插件侧补强登记为后续项。依据见下。

---

## 1. 插件体系现状能力清单（证据）

### 1.1 已具备（相当扎实）

| 能力 | 证据 |
| --- | --- |
| 协议版本带：hook 输出 `protocol_version` 必须以 `1.` 开头，`2.0` 被拒；`manifest_version` ≠ "1.0" 拒绝注册 | `src/plugin/protocol.rs:101-107`、`src/plugin/registry.rs:170-175` |
| 四段权威 schema（pluginDirectives / directiveSyntax / builtinMacros / manifestSchema），`rupost plugin schema` 输出供 IDE | `src/cli/plugin_route.rs:351-368` |
| 作者指南与权威档案（E-1/E-2 已交付） | `doc/architecture/features/plugin_and_extensibility.md` §3 |
| 插件命令通道：argv 直通 + stdio 三向继承（支持 TTY/交互）+ 环境白名单 + `RUPOST_WORKSPACE`/`RUPOST_CONFIG_DIR` 上下文 | `src/cli/plugin_exec.rs:156-221` |
| 65+ 插件契约测试（driver 11 / registry 18 / pipeline 13 / degradation 7 / dod 4 / cli_route 10） | `tests/plugin_*.rs`、`tests/cli_plugin_route_test.rs` |
| 安全防线：TOFU 命令指纹（0600、拒 symlink）、发现零子进程、目录可信校验、env 白名单、10MB IPC 上限、5s 进程组熔断 | `plugin_exec.rs:88-136`、`registry.rs:332-410`、`plugin/driver.rs` |
| 降级三态语义（默认阻断 / `?optional`+`--allow-missing-plugins` 自愈 / `--strict-plugins` 判败） | `tests/plugin_degradation_test.rs` |

### 1.2 不具备（决定性缺口）

| 缺口 | 证据 |
| --- | --- |
| **import 管线无扩展点**：`ImportFormat` 是封闭 serde 枚举；`MultiFormatImporter::import` 对 `Har/OpenApi/Postman` 落入 `other =>` 回退**按 RawHttp 尝试**并告警；importer 目录零 plugin 引用 | `src/importer/mod.rs:28-35, 127-133` |
| **export 管线无扩展点**：`ExporterRegistry::new_builtin()` 硬编码 curl/httpie/wget；`register()` 虽是 pub Rust API 但无任何从 manifest/目录加载外部 exporter 的代码路径；`--list` 只列内置三个 | `src/exporter/mod.rs:14, 17-27, 37-42`、`src/exporter/plugin.rs:29-37` |
| **无安装/发布/校验命令**：`plugin sync` 已裁撤，仅剩 `List/Schema`；安装 = 手动拷贝"二进制 + manifest"到搜索目录 | `src/cli/commands/mod.rs:104-111`、`plugin_and_extensibility.md:69-71` |
| **命令插件拿不到 rupost 内部库**：命令 = 外部进程，无进程内 API；`.http/.md` 解析器、变量上下文均不可复用 | `plugin_exec.rs:189-212` |
| **hook 与 import/export 零关联**：hook 仅存在于发包前的请求管线（prepare/transform/sign），不能改 URL/method、不能消费响应 | `executor.rs:428-452`、`patch.rs:5-27` |
| **文档漂移两处**：03 设计文档声称注入 `RUPOST_ENV`（代码未实现）；`export --list` 的"export plugins"措辞实为**内置 target**，易误导第三方作者 | `03-...md §6.2` vs `plugin_exec.rs:194-205`；`src/exporter/mod.rs:18` |

---

## 2. 两项功能的插件化可行性逐项分析

### 2.1 `import openapi` → 插件承载？

**功能本质**：纯输入→输出转换（JSON spec → Markdown 文本），**不需要 rupost 内部任何能力**（不用解析器、不用变量上下文、不碰网络）。

| 形态 | 可行性 | 说明 |
| --- | --- | --- |
| **命令插件**（如 `rupost-plugin-openapi2md`，用户执行 `rupost openapi2md spec.json > api.md`） | ✅ **今天即可满足** | 命令通道 argv/stdio 直通、任意语言实现；TOFU 保障首次执行安全；防覆盖由插件自行实现 |
| `rupost import openapi` 一等公民形态 | ❌ 不满足 | `import` 是内置命令，子命令分发不经过插件路由（插件路由只拦截**顶层未知命令**，`plugin_route.rs:88-110`）；且 importer 枚举封闭 |
| hook 形态 | ❌ 无关 | hook 只在请求管线，不碰 import |

**结论：** 第三方今天就**能**用插件实现"OpenAPI → Markdown 转换工具"，基础条件满足；但它不会出现在 `rupost import` 的能力面里，用户须感知两个命令入口。

### 2.2 `export k6` → 插件承载？

**功能本质**：`.http/.md` → k6 JS 骨架。转换的**输入语义强依赖核心解析器**——URL/headers/body 的 `{{var}}` 约定、`@assert` 语义、`@skip`/teardown 排除、`@capture`/`@depends-on` 提示，全部是 rupost 的领域约定。

| 形态 | 可行性 | 说明 |
| --- | --- | --- |
| 命令插件（第三方自研 .http 解析 → JS） | ⚠️ 技术可行但**质量分叉** | 插件拿不到核心解析器，需自行重实现解析——断言语义、变量约定、skip/teardown 语义极易与核心漂移；`rupost export --list` 也发现不了它 |
| `rupost export k6` 一等 target | ❌ 不满足（插件侧） | `ExporterRegistry` 编译期封闭，无外部加载路径；要开放需核心侧新增"export 插件加载 + manifest 声明 target"的扩展点设计 |
| Rust 库依赖形态（插件以 Rust 写、依赖 rupost crate 复用解析器） | ⚠️ 理论可行、未产品化 | rupost 是 lib crate，但从未以"工具作者 SDK"定位维护过（公共 API 稳定性无承诺），登记为远期选项 |

**结论：** k6 导出**不适合**交由第三方插件承载——解析语义分叉风险直接损害"文档即契约"定位；基础条件（管线扩展点 + 解析器复用通道）不满足。

> **追加评估（AST 桥）：** 上述"解析语义分叉"障碍可以通过向插件提供核心解析结果（AST JSON 桥）解除——详见 §6。解除后 k6 的插件路径质量风险消除，但 `rupost export k6` 的一等 UX（`--list` 发现、旗标一致性）仍只有核心内置能提供；对执行计划的影响见 §6.4。

---

## 3. 基础条件满足度矩阵（面向"让其他人用 plugin 实现功能"）

| 条件 | 状态 | 缺口动作 |
| --- | --- | --- |
| C1 协议稳定 + 版本字段 | ✅ 满足 | — |
| C2 权威 manifest schema + 作者文档 | ✅ 满足 | 修复 §1.2 两处文档漂移 |
| C3 命令通道可承载任意 CLI 工具 | ✅ 满足 | — |
| C4 命令插件产出被 `import`/`export` 管线消费 | ❌ 不满足 | 需核心侧扩展点设计（见 §4 路径 C） |
| C5 安全分发（TOFU/白名单/权限） | ✅ 满足（CI 非 TTY 需先交互确认一次，属刻意 fail-closed） | — |
| C6 安装/发布/校验命令 | ❌ 缺失（手动拷贝） | `plugin validate` 可低成本补（manifest 校验逻辑已存在于发现期） |
| C7 命令型插件示例与脚手架 | ⚠️ 部分 | `examples/plugins/` 只有 hook 型（echo-sign/smoke-auth）；命令型无示例、无 `plugin new` 模板 |
| C8 测试基座（插件作者可用的契约测试工具） | ⚠️ 部分 | 核心侧 65+ 测试是"测内核"不是"给作者用"；作者需自行测 |

**总评：插件"运行时基座"满足度约 80%（C1/C2/C3/C5 齐），但"生态基座"（C4/C6/C7/C8）不满足——第三方能写出工具，但工具进不了产品能力面，也缺少示例与安装体验。**

---

## 4. 路径建议

| 路径 | 内容 | 代价 | 建议 |
| --- | --- | --- | --- |
| **A. 维持现计划（推荐）** | WP-P3a/P3b 照常核心内置；插件侧仅登记两项后续小任务：① 修复 §1.2 两处文档漂移（`RUPOST_ENV`、`export --list` 措辞）② `examples/plugins/` 补一个**命令型**最小示例（manifest + sh 脚本），与 WP-R4 文档收尾合并 | 近零 | ✅ 采纳。理由：`ImportFormat::OpenApi` 检测已存在，核心实现让 CLI+Web 双入口同批受益；k6 强依赖解析器语义，核心内置是唯一不分叉的形态 |
| B. 插件承载 + 管线扩展点 | 设计"import 格式插件 / export target 插件"manifest 扩展（声明式格式注册 + stdin/stdout 约定） | 大：扩展点设计 + 安全审查 + 两套管线改造，且与竞争报告"不为尚无用户需求的协议或插件面扩大公开 API"的纪律冲突 | ❌ 现在不做；等出现**多个**真实第三方格式需求再立项 |
| C. 插件承载（纯命令形态） | P3a/P3b 撤出核心，发布作者指南鼓励第三方写 `openapi2md`/`http2k6` 命令插件 | 中：省核心开发，但 k6 解析分叉、能力面割裂（`import`/`export --list` 看不到）、无安装体验 | ❌ k6 不接受；openapi 可作为社区补充而非替代 |

**对执行计划的影响：无变更。** WP-P3a/P3b 保持核心内置；§4 路径 A 的两个插件侧小任务并入 WP-R4 文档收尾的可选项，或在下一批次以独立小 WP 落地。

---

## 5. 证据索引

- 插件命令路由/TOFU/env：`src/cli/plugin_route.rs`、`src/cli/plugin_exec.rs`；测试 `tests/cli_plugin_route_test.rs`
- hook IPC 协议与边界：`src/plugin/{pipeline,patch,protocol,driver,types}.rs`；`src/runner/executor.rs:428-452`
- import 封闭枚举与 RawHttp 回退：`src/importer/mod.rs:28-35, 127-133`
- export 内置注册表：`src/exporter/mod.rs:14-42`、`src/exporter/plugin.rs:29-37`
- 权威档案与作者指南：`doc/architecture/features/plugin_and_extensibility.md`；协议 SSOT：`doc/plans/2026-11-10-cross-scenario-engine-and-plugins/03-plugin-spi-and-degradation.md`

---

## 6. 追加评估：可否提供 AST 供插件使用？

**结论：可以，推荐"`rupost ast` JSON 桥"形态（形态 A）**——成本小、零数据访问面扩大、以既有契约纪律即可守住稳定性；已完成代码事实核验并写入执行计划为使能工作包 **WP-AST**。

### 6.1 三种形态对比

| 形态 | 做法 | 代价 | 裁决 |
| --- | --- | --- | --- |
| **A. `rupost ast <FILE>` JSON 桥** | 核心新增只读命令：解析文档 → 稳定 JSON（AstDocument v1.0）打 stdout；插件命令内经 `RUPOST_BIN` 自调内核二进制获取 AST | 小：投影结构 + 一个命令 + 2 行 env 注入 | ✅ **采纳** |
| B. stdin AST 注入协议 | manifest command 声明 `input: "ast"`，内核把 AST JSON 管进插件 stdin | 大：命令模式今天是**无协议版本的裸 argv 直通**（stdin 继承 TTY），注入结构化 stdin 与 `requires_tty` 冲突，需版本化协议 + schema 扩展 | ❌ 形态 A 以更小代价达成同一目标 |
| C. Rust 库 SDK | 插件以 Rust 依赖 rupost crate 直接调 parser | 大：公共 API 稳定性承诺、跨语言作者被排除 | ❌ 远期选项，登记不做 |

### 6.2 形态 A 的关键事实（已核验）

- **安全面零扩大**：命令插件本就能读工作区里的 `.http/.md` 原文（`RUPOST_WORKSPACE` 已知、文件在盘上）；AST 桥只是**共享解析代码**，不新增任何数据访问。
- **解析器类型现状**：`ParsedFile/ParsedRequest/RequestMetadata/ParsedVariableCapture` 均**未实现 Serialize**（仅 FileMetadata 有，`src/parser/types.rs`）——因此**不做直接 derive**（会把内部命名泄漏成公共契约），而是按项目既有契约纪律（snapshot/incident/contract 均如此）建**专用投影结构 `AstDocument`，`schema_version "1.0"`，只增不改**。
- **环境注入点现成**：`plugin_exec.rs:195-211` 白名单重建处追加 `RUPOST_BIN = std::env::current_exe()`（约 2 行），插件即可 `rupost "$RUPOST_BIN" ast file.http` 自调内核。
- **依赖关系**：AST 暴露 `source_line`，故 WP-AST 排在 WP-P2a（来源行号）之后。

### 6.3 对 §2.2 结论的修订

AST 桥落地后，§2.2 中"k6 插件路径质量分叉"的障碍解除：第三方（或一方 showcase 插件）可基于 `rupost ast` 生成 k6 脚本，**解析语义与核心零分叉**。但两点维持不变：① `rupost export k6` 的一等 UX（`--list` 发现、旗标一致性）仍只有核心内置能提供；② WP-P3b 默认仍为核心内置——AST 桥的价值定位是**插件生态使能器**（后续可做一方 showcase 插件验证"他人用插件实现功能"的完整闭环），不是 P3b 的替代。

### 6.4 执行影响

- 执行计划新增 **WP-AST**（使能工作包，排在 WP-P2a 之后）：`rupost ast` 命令 + `AstDocument v1.0` 投影 + `RUPOST_BIN` 注入 + 契约测试 + 作者指南补节。
- WP-P3a/P3b 结论不变；"一方 showcase 插件"登记为 C-R4 之后的实验项。

---

## 7. AST 桥使用方案（插件作者视角）

### 7.1 端到端时序

```text
用户                          内核（rupost）                      插件（rupost-plugin-k6gen）
 │ rupost k6gen suite.http      │                                   │
 │────────────────────────────>│ 路由：顶层未知命令 → 插件命令        │
 │                              │ TOFU 首跑确认（路径+SHA-256 指纹）  │
 │                              │ env_clear + 白名单重建             │
 │                              │ 注入 RUPOST_BIN / RUPOST_WORKSPACE │
 │                              │ exec（argv 业务参数透传）──────────>│
 │                              │                                   │ 读 suite.http（工作区在盘上）
 │                              │<── $RUPOST_BIN ast suite.http ────│ （子进程，capture stdout）
 │                              │ AstDocument JSON ────────────────>│
 │                              │                                   │ 校验 schema_version 前缀
 │                              │                                   │ 生成 k6 脚本 → 自己的 stdout
 │ <──────────────────────────── k6 脚本（插件 stdio 直通给用户）──────│
```

要点：**插件是普通进程**——argv 业务参数直通、stdio 三向继承、退出码穿透；`ast` 子进程的 stdout 必须被插件**捕获**（绝不让它漏进插件自己的 stdout 污染产物）；插件自身的日志一律走 stderr。

### 7.2 AstDocument 字段使用手册（v1.0）

| 字段 | 类型 | 语义 | 插件典型用法 |
| --- | --- | --- | --- |
| `schema_version` | string | 数据契约版本（当前 `"1.0"`） | **第一步**校验前缀 `"1."`，否则可读失败（§8） |
| `source_path` | string | 解析时的（相对）路径 | 生成物头部署名、错误定位 |
| `dependencies` | string[] | 文件级 `@depends-on` 原始声明 | 生成注释提示（执行排序是 runner 职责） |
| `requests[].number` | usize | 文件内 1-based 序号 | 注释/命名（`res1`） |
| `requests[].method` / `.url` | string | **未插值原文**（`{{var}}` 原样保留） | `{{base_url}}` → `` `${BASE_URL}` ``；其余 `{{var}}` → `__ENV.VAR` |
| `requests[].headers` | [string,string][] | 保序键值对 | `params.headers` |
| `requests[].body` | string? | 原始请求体 | `params.body` |
| `requests[].name` | string? | `@name` | 注释/check 标签前缀 |
| `requests[].skip` | bool | `@skip` | `true` 则跳过不生成 |
| `requests[].assertions` | string[] | 断言原文（如 `"status == 200"`） | 仅 `status == N` 映射 `check()`；其余 `// TODO` 注释 |
| `requests[].captures` | string[] | `@capture` 原始声明 | 注释提示"k6 变量传递需手动编排" |
| `requests[].source_line` | usize | 方法行 1-based 行号 | 生成注释 `// suite.http:3`（回溯文档） |

**容忍规则**：顶层与 `requests[]` 都可能**新增字段**（1.x 加法兼容）——插件按已知 key 白名单取用，绝不枚举校验整个对象；可选字段缺席时降级而非崩溃。

### 7.3 最小可用插件 walkthrough（k6gen 示例）

**步骤 1 —— 写可执行文件**（与 manifest 同目录、同名的普通文件，任意语言；下例 python3，Windows 需 `.cmd` 薄包装）`rupost-plugin-k6gen`：

```python
#!/usr/bin/env python3
import json, os, re, subprocess, sys

def fail(msg, code=1):
    print(msg, file=sys.stderr); sys.exit(code)

def main():
    args = sys.argv[1:]
    if not args:
        fail("usage: rupost k6gen <FILE> [--out out.js]")
    doc = args[0]
    out = args[args.index("--out") + 1] if "--out" in args else None

    # 版本协调第一原则：用内核注入的 RUPOST_BIN（见 §8.3-1），不裸调 rupost
    rupost = os.environ.get("RUPOST_BIN", "rupost")
    if not os.path.exists(doc):
        fail(f"file not found: {doc}")

    proc = subprocess.run([rupost, "ast", doc], capture_output=True, text=True, timeout=10)
    if proc.returncode != 0:
        fail(f"ast failed: {proc.stderr.strip()}")

    ast = json.loads(proc.stdout)
    sv = str(ast.get("schema_version", ""))
    if not sv.startswith("1."):
        fail(f"AST schema {sv} incompatible; need 1.x — 调整 rupost/插件版本")

    lines = [
        f"// generated by rupost-plugin-k6gen (AST schema {sv})",
        "import http from 'k6/http';", "import { check } from 'k6';",
        "export const options = { vus: 1, iterations: 1 }; // 负载模型自行扩展",
        "const BASE_URL = __ENV.BASE_URL || '';",
        "export default function () {",
    ]
    n = 0
    for req in ast.get("requests", []):
        if req.get("skip"):
            continue
        n += 1
        url = req["url"].replace("{{base_url}}", "${BASE_URL}")
        url = re.sub(r"\{\{(\w+)\}\}", lambda m: f"__ENV.{m.group(1).upper()}", url)
        lines.append(f"  // {doc}:{req.get('source_line', '?')} {req.get('name') or ''}")
        lines.append(f"  const res{n} = http.get(`{url}`);")
        for a in req.get("assertions", []):
            if a.startswith("status == "):
                code = a.split("==")[1].strip()
                lines.append(f"  check(res{n}, {{ '{a}': (r) => r.status === {code} }});")
            else:
                lines.append(f"  // TODO: @assert {a}")
        for c in req.get("captures", []):
            lines.append(f"  // TODO: @capture {c} — k6 变量传递需手动编排")
    lines.append("}")
    script = "\n".join(lines) + "\n"
    open(out, "w").write(script) if out else sys.stdout.write(script)

main()
```

**步骤 2 —— 写同名静态 manifest** `rupost-plugin-k6gen.json`：

```json
{
  "manifest_version": "1.0",
  "name": "k6gen",
  "version": "0.1.0",
  "description": "Generate a k6 scenario draft from .http/.md documents",
  "commands": [
    { "name": "k6gen", "usage": "rupost k6gen <FILE> [--out out.js]",
      "description": "AST 桥示例：读取文档 AST 生成 k6 场景草稿", "requires_env": [] }
  ]
}
```

硬性校验提醒：description ≤80 字符；可执行文件必须与 manifest 同目录；`ast` 无需任何 `requires_env`。

**步骤 3 —— 安装与首跑**：两个文件一起拷入 `~/.rupost/plugins/`（或 `$PATH` 目录；项目 `./.rupost/plugins/` 需 `--allow-project-plugins` 放行）→ `rupost plugin list` 应显示 ready → `rupost --help` 出现 Extensions 区块 → 首次 `rupost k6gen suite.http` 在交互终端确认 TOFU（`y`），之后指纹命中静默放行；**CI/非 TTY 环境必须先在交互终端确认一次**，否则 fail-closed。

### 7.4 陷阱清单

- 内层 `ast` 调用必须 `capture_output`——插件 stdout 只允许出现最终产物（stdout 直通给用户）。
- 自备子进程超时（如 10s）：命令模式没有 hook 的 5s 熔断，防挂死是插件自己的责任。
- `ast` 是只读命令：不发网络、不写 history、不产生复现包——无需处理 `RUPOST_HISTORY_DIR`。
- 不要缓存 AST 跨运行/跨版本：每次现取（§8.3-6）。

---

## 8. 版本更新如何协调

### 8.1 三个版本轴（互不相干，勿混淆）

| 轴 | 当前值 | 归属 | 兼容承诺 |
| --- | --- | --- | --- |
| `manifest_version` | 1.0 | 插件清单格式 | ≠ "1.0" 拒绝注册 |
| `protocol_version` | 1.0 | **hook IPC 通道**（与 AST 无关） | hook 输出必须 `"1."` 前缀 |
| **AstDocument `schema_version`** | **1.0** | **AST JSON 数据契约** | **1.x 内只增不改；主版本升级 = 破坏性变更** |

AST 桥只涉及第三轴；但插件同时自带第一轴的 manifest。**命令通道本身没有协议版本——这不是缺陷而是设计**：`RUPOST_BIN` 注入后，插件面对的 AST 生产者就是用户正在运行的那个 rupost 二进制，天然同版本、零漂移（单一二进制真相源）。

### 8.2 核心侧承诺（1.x 内）

1. **字段只增不改**：已有字段的名字/类型/语义冻结；新信息一律以新字段追加。
2. **不删字段**：弃用时保留输出并标注 deprecated（或值恒 null），跨一个主版本才允许移除。
3. **语义稳定**：`source_line` 永远是"方法行 1-based 行号"、`url` 永远是未插值原文——不悄悄改含义。
4. `schema_version` 单调：1.x → 2.0 仅在不得不破坏时发生，发布说明列明迁移清单。
5. 演进载体：AstDocument 投影层隔离内部类型——`ParsedFile` 内部重构不得破坏契约（WP-AST §2.7 已锁定此纪律）。

### 8.3 插件侧协调守则（作者清单，六条）

1. **永远用 `RUPOST_BIN`，不用裸 `rupost`**——杜绝 PATH 上陈旧二进制造成的版本漂移（这是唯一可能"插件和生产者不同版本"的通道）。
2. 读到 AST 后**第一步校验 `schema_version` 前缀** `"1."`；不符则可读失败："AST schema 2.0 incompatible; need 1.x"。
3. **容忍未知字段**：顶层与 `requests[]` 均按已知 key 白名单取用，不整对象枚举校验（加法兼容的前提）。
4. **缺席即降级**：可选字段缺席（生产者更旧）时用占位/跳过，不崩溃。
5. README 登记兼容声明："AST schema 1.x（rupost 0.2.x 实测）"；缺陷报告附 `rupost --version`。
6. **不缓存 AST 跨运行/跨版本**：每次现取，AST 是派生数据不是存储格式。

### 8.4 三种错位场景与预期行为

| 场景 | 表现 | 结果 |
| --- | --- | --- |
| 旧插件 + 新 rupost（1.x 增字段） | 新字段被白名单取用逻辑自然忽略 | ✅ 正常工作（加法兼容） |
| 新插件 + 旧 rupost（期望字段缺席） | 字段缺席检测 | 降级或可读报错"需要 rupost ≥ X" |
| 任一 1.x 插件 + 2.0 rupost | 前缀校验失败 | 可读失败并指引升级插件/降级 core（破坏性变更本就该显式暴露，而非静默错译） |

### 8.5 升级编排（运维视角）

- **rupost 升级** = 替换单个二进制；AST 生产者随之更新，1.x 插件零改动。
- **插件升级** = 替换插件目录内两个文件；registry 缓存按"mtime+SHA-256 指纹"自动重建，无需手动失效。
- **回滚**：两者各自独立回滚；二进制变化 → TOFU 指纹变化 → 首次执行重新确认一次（预期行为，插件发布说明必须写明）。
- **CI 提示**：非 TTY 环境的 TOFU 确认是一次性的（指纹落盘 0600）；升级插件后 CI 首跑会 fail-closed，需先在交互终端确认。
