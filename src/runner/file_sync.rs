use std::path::PathBuf;
use crate::Result;

pub struct FileSyncWriter {
    target_path: PathBuf,
    append: bool,
    buffer: String,
}

impl FileSyncWriter {
    pub fn new(path: PathBuf, append: bool) -> Self {
        Self {
            target_path: path,
            append,
            buffer: String::new(),
        }
    }

    pub fn write_delta(&mut self, delta: &str, _provider_name: &str) -> Result<()> {
        self.buffer.push_str(delta);
        // MVP 骨架暂时不执行真正物理写入，仅返回 OK
        Ok(())
    }
}
