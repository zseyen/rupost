//! `rupost import`：外部格式导入子命令枚举与 curl 参数。

use clap::{Parser, Subcommand};

#[derive(Subcommand)]
pub enum ImportCommands {
    /// Import a curl command into an executable .http request block
    Curl(ImportCurlArgs),
}

#[derive(Parser, Debug)]
pub struct ImportCurlArgs {
    /// curl 命令：整体引号包裹，或直接粘贴为多参数（自动空格拼接）
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CURL"
    )]
    pub curl: Vec<String>,

    /// 写入 .http 文件（已存在则拒绝）；缺省打印 stdout
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<String>,

    /// origin 模板化为 {{base_url}}（跨环境运行）
    #[arg(long)]
    pub templatize_host: bool,
}

impl ImportCurlArgs {
    pub fn into_request(self) -> rupost::importer::curl::CurlImportRequest {
        // trailing_var_arg 拦截（AGENTS §5 同源陷阱）：位置参数之后的 -o/--output/--out
        // 与 --templatize-host 属 rupost 侧开关——从 CURL 序列中摘出。
        // 已知取舍：curl 自身的 -o（响应落盘）语义在此被覆盖——devtools 复制的
        // curl 不含 -o，真实冲突罕见；现有测试与 CHANGELOG 契约依赖本拦截，保留。
        let mut output = self.output;
        let mut templatize_host = self.templatize_host;
        let mut kept: Vec<String> = Vec::with_capacity(self.curl.len());
        let mut it = self.curl.into_iter().peekable();
        while let Some(tok) = it.next() {
            match tok.as_str() {
                "-o" | "--output" | "--out" => {
                    if let Some(v) = it.next() {
                        output = Some(v);
                    }
                }
                "--templatize-host" => templatize_host = true,
                other => kept.push(other.to_string()),
            }
        }

        rupost::importer::curl::CurlImportRequest {
            raw: kept.join(" "),
            output,
            templatize_host,
        }
    }
}
