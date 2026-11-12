use crate::Result;
use std::path::{Path, PathBuf};

/// 判断路径是否为受支持的活文档（`.http` / `.md` 等），SSOT 委托给 `path_guard::is_live_doc`。
pub fn is_supported_doc(path: &Path) -> bool {
    crate::utils::path_guard::is_live_doc(path)
}

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
                if is_supported_doc(&canonical) {
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
                if is_supported_doc(&path) {
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
