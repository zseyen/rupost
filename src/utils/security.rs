use crate::{Result, RupostError};
use std::path::Path;

pub fn run_security_lint(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;

    // Detect typical API keys (e.g. starting with 'sk-' followed by at least 12 alphanumeric characters)
    let re = regex::Regex::new(r"sk-[a-zA-Z0-9]{12,}").unwrap();

    for (idx, line) in content.lines().enumerate() {
        if re.is_match(line) {
            return Err(RupostError::Other(format!(
                "[SECURITY ALERT] Leaked plain-text API key detected in {:?} at line {}: {}",
                path,
                idx + 1,
                line.trim()
            )));
        }
    }

    Ok(())
}
