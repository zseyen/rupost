use crate::Result;
use std::path::PathBuf;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

pub struct FileSyncWriter {
    target_path: PathBuf,
    append: bool,
    file: Option<File>,
    header_written: bool,
}

impl FileSyncWriter {
    pub fn new(path: PathBuf, append: bool) -> Self {
        Self {
            target_path: path,
            append,
            file: None,
            header_written: false,
        }
    }

    pub async fn write_delta(&mut self, delta: &str, _provider_name: &str) -> Result<()> {
        if self.file.is_none() {
            // Ensure parent directory exists
            if let Some(parent) = self.target_path.parent().filter(|p| !p.exists()) {
                tokio::fs::create_dir_all(parent).await?;
            }

            let file = if self.append {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.target_path)
                    .await?
            } else {
                let mut f = File::create(&self.target_path).await?;
                f.write_all(b"# LLM Prompt Debugging Report\n\n").await?;
                self.header_written = true;
                f
            };
            self.file = Some(file);
        }

        if let Some(ref mut f) = self.file {
            f.write_all(delta.as_bytes()).await?;
            f.flush().await?;
        }

        Ok(())
    }

    /// 从 SSE 帧事件数据中提取 LLM 文本碎片（delta）并写入本地文件
    pub async fn write_sse_event(
        &mut self,
        event_data: &str,
        llm_provider: crate::http::llm_adapter::LlmProvider,
    ) -> Result<Option<String>> {
        let llm_adapter = crate::http::llm_adapter::LlmStreamAdapter;
        if let Some(delta) = llm_adapter.extract_delta(llm_provider, event_data) {
            self.write_delta(&delta, &format!("{:?}", llm_provider))
                .await?;
            Ok(Some(delta))
        } else {
            Ok(None)
        }
    }
}

/// 流式日志截断标记（SSOT 常量：`tui/state/retention.rs` 端口版同引此值，消除句号分叉）
pub const LOG_TRUNCATED_MARKER: &str = "\n[SYSTEM] Log truncated due to size limit.\n";

/// 针对流式事件日志的安全追加写入函数：
/// 1. 自动确保父级目录存在；
/// 2. 检测当前文件大小，达到上限则静默跳过写入；
/// 3. 本次写入若导致文件超限，写入截断提示信息并阻止超出部分。
pub async fn write_log_with_limit(
    log_path: &std::path::Path,
    data: &str,
    max_bytes: usize,
) -> std::io::Result<()> {
    if let Some(parent) = log_path.parent().filter(|p| !p.exists()) {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let file_len = log_path.metadata().map(|m| m.len() as usize).unwrap_or(0);
    if file_len >= max_bytes {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(log_path)
        .await?;

    let new_len = file_len + data.len();
    if new_len > max_bytes {
        // 截断提示是 best-effort 附加信息，写失败不阻断主流程；
        // flush 恢复串行化——防止下一次 metadata 采样时 marker 未落盘导致重复写
        let _ = file.write_all(LOG_TRUNCATED_MARKER.as_bytes()).await;
        let _ = file.flush().await;
    } else {
        // 数据写失败必须上抛——外层 SSE/WS 的 once 告警闸门依赖此 Err
        // （审查 P0-A：此前 let _ = 吞错返回 Ok，闸门只保护到 open 为止）
        file.write_all(data.as_bytes()).await?;
        file.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_log_with_limit_truncation() {
        let temp = tempfile::tempdir().unwrap();
        let log_path = temp.path().join("sub/test.log");

        // 第一次写入正常
        write_log_with_limit(&log_path, "12345", 20).await.unwrap();
        let content = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert_eq!(content, "12345");

        // 写入超限触发截断提示
        write_log_with_limit(&log_path, "this-is-too-long-data-exceeding-limit", 20)
            .await
            .unwrap();
        let content2 = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert!(content2.contains("[SYSTEM] Log truncated"));

        // 超限后再次写入不再添加内容
        let len_before = tokio::fs::metadata(&log_path).await.unwrap().len();
        write_log_with_limit(&log_path, "more data", 20)
            .await
            .unwrap();
        let len_after = tokio::fs::metadata(&log_path).await.unwrap().len();
        assert_eq!(len_before, len_after);
    }
}
