use crate::Result;
use crate::runner::path::is_supported_file;
use std::path::{Path, PathBuf};

pub struct DirectoryScanner;

impl DirectoryScanner {
    pub fn scan(paths: &[String]) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for path_str in paths {
            let path = Path::new(path_str);
            if !path.exists() {
                return Err(crate::error::RupostError::Other(format!(
                    "路径不存在: {}",
                    path_str
                )));
            }
            let canonical = path.canonicalize()?;
            if canonical.is_file() {
                if is_supported_file(&canonical) {
                    files.push(canonical);
                }
            } else if canonical.is_dir() {
                Self::scan_dir(&canonical, &mut files)?;
            }
        }

        // 字典序排序并去重
        files.sort();
        files.dedup();

        Ok(files)
    }

    fn scan_dir(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        let read_dir = std::fs::read_dir(dir)?;
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();

            // 排除隐藏文件、隐藏目录，以及 target, node_modules 等常见忽略目录
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
                    continue;
                }
            }

            if path.is_file() {
                if is_supported_file(&path) {
                    let canonical = path.canonicalize()?;
                    files.push(canonical);
                }
            } else if path.is_dir() {
                Self::scan_dir(&path, files)?;
            }
        }
        Ok(())
    }
}
