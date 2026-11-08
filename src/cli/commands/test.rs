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

    /// 报告输出格式 (terminal, json 或 html)
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

    /// 只运行单个请求：文件内序号 (3) 或 @name (@login)；也可用 path#3 片段语法
    #[arg(long, value_name = "N|@NAME")]
    pub only: Option<String>,

    /// 失败时执行的 shell 命令（每个失败请求一次；env: RUPOST_FAILED_URL/STATUS/TRACE_ID）
    #[arg(long, value_name = "CMD")]
    pub on_failure: Option<String>,
}
