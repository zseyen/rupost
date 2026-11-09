# .agents 目录说明

本目录作为 IDE 与 Agent 工具链的兼容镜像目录存在：

1. **规则单一事实来源 (Rule SSOT)**：
   - 权威规则位于项目根目录的 [AGENTS.md](../AGENTS.md)；
   - 本目录下的 [AGENTS.md](./AGENTS.md) 为物理同步镜像，确保任何加载 `.agents/AGENTS.md` 的环境均具备 100% 一致的规则约束。

2. **技能单一事实来源 (Skills SSOT)**：
   - 全体 8 大 Agent Skills 的权威存放路径为 [.agent/skills/](../.agent/skills/)；
   - 本目录下的 `skills/` 通过符号链接指向 `.agent/skills/`，保持向下兼容。
