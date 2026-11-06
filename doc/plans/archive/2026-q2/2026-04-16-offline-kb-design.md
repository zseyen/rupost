# Rupost 离线知识库系统设计方案

## 1. 核心模型 (Core Models)

### 1.1 KnowledgePoint (原子知识点)
- **匹配模式 (Pattern)**: 支持 URL 正则、Header 关键字匹配。
- **真相提示 (Truth)**: 专家级的避坑指南字符串。
- **关联场景 (Context)**: 该知识点通常出现的业务上下文。

### 1.2 Scenario (场景定义)
- **步骤 (Steps)**: 引用一组历史请求记录（Snapshot ID）。
- **指引 (Guide)**: 每一阶段的人类可读说明。
- **预期 (Expectation)**: 成功的断言特征。

## 2. 存储策略
- 目录：`.rupost/kb/`
- 格式：
  - `knowledge.jsonl`: 存储所有原子知识点。
  - `scenarios/*.md`: 存储场景引导文件（Markdown 格式，带 Frontmatter 描述逻辑）。

## 3. 功能模块

### 3.1 蒸馏引擎 (Distiller)
- 交互式选择：让用户从历史记录中勾选并合并为场景。
- 模式提取：自动寻找成功请求相比失败请求的差分项（比如补上的某个 Header）。

### 3.2 影子运行 (Shadow Runner)
- `--guide` 模式：在执行时，不仅显示输出，还在侧边/下方参考区显示对应的历史“标准案例”。

### 3.3 知识匹配中间件 (Kb Middleware)
- 在 `before_request` 阶段静默匹配，发现潜在风险（如 URL 匹配了某个 Trap）时打印警告。

## 4. 实施阶段 (Roadmap)
- **MVP**: 实现 `KnowledgePoint` 存储和 CLI 警告提醒。
- **V1.1**: 实现手动蒸馏命令 `rupost distill`。
- **V1.2**: 实现 `--guide` 模式及侧边参考。
