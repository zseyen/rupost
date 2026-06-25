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
}
