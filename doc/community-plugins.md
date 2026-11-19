# 社区插件索引

> 纯 PR 维护的轻量索引（零基建）：想收录你的插件，提交 PR 在下表加一行即可。
> 作者指南：[插件与扩展](architecture/features/plugin_and_extensibility.md) §3
> （两种插件形态矩阵、命令插件五步、AST 桥）；AST 桥版本协调守则见
> `architecture/executed_plans_digest.md` §2.3（原评估文档已归 git 历史）。

| 插件 | 形态 | 一句话 | 链接 | 维护者 |
| :-- | :-- | :-- | :-- | :-- |
| cmd-hello（官方样板） | command | argv 直通与内核上下文注入最小示例 | `examples/plugins/` | rupost |
| echo-sign（官方样板） | hook | `@sign` 请求签名管线示例（Stdio IPC） | `examples/plugins/` | rupost |
