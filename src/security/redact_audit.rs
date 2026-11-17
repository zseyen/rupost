//! 脱敏审计日志（批三-6：自 redact.rs 抽出的独立职责——记录 + 豁免 + footer 渲染）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionSource {
    Header,
    UrlQuery,
    UrlFragment,
    BodyJson,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionAuditEntry {
    pub key: String,
    pub scheme: Option<String>,
    pub raw_len: usize,
    pub source: RedactionSource,
}

#[derive(Debug, Clone, Default)]
pub struct RedactionAuditLog {
    entries: Vec<RedactionAuditEntry>,
    /// 豁免审计（审查批二 C-b）：`@no-redact` 声明且基础策略本会命中的键——
    /// 值保留原样出站，但脚注如实记录"谁被豁免了"，杜绝静默免脱敏
    exempted: Vec<String>,
}

impl RedactionAuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一条豁免（去重）：值保留原样，但脚注可追溯
    pub fn record_exemption(&mut self, key: &str) {
        if !self.exempted.iter().any(|k| k == key) {
            self.exempted.push(key.to_string());
        }
    }

    pub fn record(
        &mut self,
        key: &str,
        scheme: Option<&str>,
        raw_len: usize,
        source: RedactionSource,
    ) {
        self.entries.push(RedactionAuditEntry {
            key: key.to_string(),
            scheme: scheme.map(|s| s.to_string()),
            raw_len,
            source,
        });
    }

    pub fn entries(&self) -> &[RedactionAuditEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 渲染脱敏清单 Footer（必须输出至 stderr，确保 stdout 命令纯净）
    pub fn render_footer<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        if self.entries.is_empty() && self.exempted.is_empty() {
            return Ok(());
        }

        writeln!(
            writer,
            "\n[i] Redacted {} credential(s):",
            self.entries.len()
        )?;
        for entry in &self.entries {
            let scheme_desc = match &entry.scheme {
                Some(s) => format!("{}, ", s),
                None => String::new(),
            };
            let source_desc = match entry.source {
                RedactionSource::Header => "Header",
                RedactionSource::UrlQuery => "URL Query",
                RedactionSource::UrlFragment => "URL Fragment",
                RedactionSource::BodyJson => "Body JSON",
            };
            writeln!(
                writer,
                "  - {} ({}{} chars) [{}]",
                entry.key, scheme_desc, entry.raw_len, source_desc
            )?;
        }
        if !self.exempted.is_empty() {
            writeln!(
                writer,
                "\n[i] Exempted by @no-redact ({}): {}",
                self.exempted.len(),
                self.exempted.join(", ")
            )?;
        }
        Ok(())
    }
}
