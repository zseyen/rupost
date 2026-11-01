//! curl → rupost `.http` 导入器（通用模块，非 AI）。
//!
//! 与 `src/exporter/` 互为逆变换。确定性解析器：零 AI、零网络、离线可用。

pub mod curl;

use crate::Result;
use curl::CurlImportRequest;

/// F6 入口：导入 curl 命令 → `.http` 请求块（stdout 或 `-o` 文件，防覆盖）
pub fn run_curl_import(req: &CurlImportRequest) -> Result<()> {
    curl::run_curl_import(req)
}
