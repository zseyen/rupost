use std::path::Path;
use crate::Result;

pub fn run_security_lint(_path: &Path) -> Result<()> {
    // 骨架默认返回 OK。在 TDD 中，由于未检测并报错明文 Key，测试一将会失败，符合红灯预期。
    Ok(())
}
