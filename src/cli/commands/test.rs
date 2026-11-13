//! `rupost test`（别名 t）：批量测试执行参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct TestArgs {
    /// Paths to .http/.md files or directories containing them
    #[arg(required = true, value_name = "PATHS")]
    pub paths: Vec<String>,

    /// 执行模式 (sequential 或 parallel)
    #[arg(long, default_value = "sequential")]
    pub mode: String,

    /// 并行模式下的并发数
    #[arg(long, default_value = "4")]
    pub concurrency: usize,

    /// 报告输出格式 (terminal, json, html 或 junit/xml)
    #[arg(long, default_value = "terminal")]
    pub report: String,

    /// 将测试报告额外写入指定文件（支持 .json 或 .html 自动识别，不干扰终端输出）
    #[arg(long, value_name = "FILE")]
    pub report_file: Option<String>,

    /// 对每个失败请求输出原位原因分析与修复建议（兼容 --ai-explain 与 RUPOST_AI_EXPLAIN=1）
    #[arg(long, visible_alias = "ai-explain")]
    pub explain: bool,

    /// 在批量测试汇总后输出失败根因聚类分诊报告
    #[arg(long)]
    pub triage: bool,

    /// 遇到第一个失败文件时立即停止
    #[arg(long)]
    pub fail_fast: bool,

    /// Environment name (e.g., dev, staging, prod)
    #[arg(short, long)]
    pub env: Option<String>,

    /// Variable overrides (key=value)
    #[arg(long, value_name = "KEY=VALUE")]
    pub var: Vec<String>,

    /// Path to a local environment file (e.g., .env)
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<String>,

    /// Show detailed request/response information
    #[arg(short, long)]
    pub verbose: bool,

    /// Disable automatic cookie handling
    #[arg(long)]
    pub no_cookies: bool,

    /// File path for persistent cookie storage
    #[arg(long, value_name = "FILE")]
    pub cookie_file: Option<String>,

    /// File path to save HTTP request/response snapshot
    #[arg(long, value_name = "FILE")]
    pub save_snapshot: Option<String>,

    /// Mask headers/body secrets in the saved snapshot (opt-in; keep off for replay body-diff fidelity)
    #[arg(long)]
    pub mask_secrets: bool,

    /// S-3 按值掩码：快照中精确命中的值替换为 [REDACTED]（可多次；独立于 --mask-secrets 生效）
    #[arg(long, value_name = "VALUE")]
    pub mask_secret_value: Vec<String>,

    /// 单响应体录入快照的上限字节（默认 262144=256KB；防 OOM，截断部分以 SHA-256 摘要验真）
    #[arg(long, value_name = "BYTES")]
    pub max_snapshot_body_bytes: Option<usize>,

    /// 时序采集模式（02 §2 契约）：仅支持 `off`——关闭时序采集，快照 timing_waterfall=null、timing_source=none
    #[arg(long, value_name = "MODE")]
    pub timing: Option<String>,

    /// 只运行单个请求：文件内序号 (3) 或 @name (@login)；也可用 path#3 片段语法
    #[arg(long, value_name = "N|@NAME")]
    pub only: Option<String>,

    /// 失败时执行的 shell 命令（每个失败请求一次；env: RUPOST_FAILED_URL/STATUS/TRACE_ID）
    #[arg(long, value_name = "CMD")]
    pub on_failure: Option<String>,

    /// `--only` 时先执行目标文件的 @depends-on 依赖闭包（免重填：上游登录/取号自动先跑）
    #[arg(long)]
    pub with_deps: bool,

    /// E-4：未解析 {{var}} 拒绝外发（默认警告 + 字面量发送；CI 可开启硬门禁）。
    /// 注意：body/URL 中合法的 {{ }} 字面量（webhook 模板等）也会被拦截
    #[arg(long)]
    pub strict_vars: bool,

    /// 捕获变量持久化文件（免重填：结束落盘 @capture/插件变量，下次运行回灌补缺；默认 .rupost/vars.json）
    #[arg(long, value_name = "FILE", num_args = 0..=1, default_missing_value = ".rupost/vars.json")]
    pub var_state: Option<String>,

    /// 插件缺失/失败时降级为警告跳过，请求按 .http 原样外发（03 §2；现场排障）
    #[arg(long)]
    pub allow_missing_plugins: bool,

    /// CI 严格门禁：发生任何插件降级 → 整体判失败 exit 1（03 §10.5）
    #[arg(long)]
    pub strict_plugins: bool,

    /// 单次插件调用超时秒数（默认 5；SigV4 STS 冷启动等慢插件可调大）
    #[arg(long, value_name = "SEC", default_value_t = 5)]
    pub plugin_timeout: u64,

    /// 显式放行插件子进程环境变量（manifest requires_env 声明外；KEY=VALUE）
    #[arg(long, value_name = "KEY=VALUE")]
    pub plugin_env: Vec<String>,

    /// 放行工作区本地 ./.rupost/plugins 插件（默认隔离，防恶意仓库 RCE）
    #[arg(long)]
    pub allow_project_plugins: bool,
}
