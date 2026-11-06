use crate::report::generator::ReportGenerator;
use crate::report::types::{ReportAssertionItem, ReportContext, TimingWaterfall, escape_html};

/// 静态 HTML 报告渲染器（零外部依赖、单文件自包含、高审美现代化 Dashboard）
pub struct HtmlReportGenerator;

impl ReportGenerator for HtmlReportGenerator {
    fn generate(&self, context: &ReportContext) -> crate::Result<String> {
        let mut html = String::with_capacity(32 * 1024);

        let title = escape_html(&context.title);
        let generated_at = escape_html(&context.generated_at);
        let env_badge = if let Some(ref env) = context.environment {
            format!(
                r#"<span class="badge badge-env">{}</span>"#,
                escape_html(env)
            )
        } else {
            String::new()
        };

        // 计算状态分布
        let sum = &context.summary;
        let pass_pct = format!("{:.1}%", sum.pass_rate);
        let pass_rate_class = if sum.failed_requests == 0 {
            "status-pass"
        } else {
            "status-fail"
        };

        html.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n");
        html.push_str("<meta charset=\"UTF-8\">\n");
        html.push_str(
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n",
        );
        html.push_str(&format!("<title>{}</title>\n", title));
        html.push_str("<style>\n");
        html.push_str(Self::embedded_css());
        html.push_str("</style>\n</head>\n<body>\n");

        // 页面主体容器
        html.push_str("<div class=\"container\">\n");

        // 1. Header
        html.push_str("  <header class=\"header\">\n");
        html.push_str("    <div class=\"brand\">\n");
        html.push_str("      <span class=\"logo\">rupost</span>\n");
        html.push_str("      <span class=\"version\">v0.2</span>\n");
        html.push_str("    </div>\n");
        html.push_str("    <div class=\"title-block\">\n");
        html.push_str(&format!("      <h1>{} {}</h1>\n", title, env_badge));
        html.push_str(&format!(
            "      <p class=\"timestamp\">生成时间: {}</p>\n",
            generated_at
        ));
        html.push_str("    </div>\n");
        html.push_str("  </header>\n\n");

        // 2. Summary KPI 卡片
        html.push_str("  <section class=\"summary-grid\">\n");
        html.push_str(&format!(
            r#"    <div class="kpi-card">
      <span class="kpi-label">通过率</span>
      <span class="kpi-val {}">{}</span>
    </div>
"#,
            pass_rate_class, pass_pct
        ));
        html.push_str(&format!(
            r#"    <div class="kpi-card">
      <span class="kpi-label">测试总数</span>
      <span class="kpi-val">{}</span>
    </div>
"#,
            sum.total_requests
        ));
        html.push_str(&format!(
            r#"    <div class="kpi-card">
      <span class="kpi-label">失败用例</span>
      <span class="kpi-val {}">{}</span>
    </div>
"#,
            if sum.failed_requests > 0 {
                "status-fail"
            } else {
                "text-muted"
            },
            sum.failed_requests
        ));
        html.push_str(&format!(
            r#"    <div class="kpi-card">
      <span class="kpi-label">总执行耗时</span>
      <span class="kpi-val">{} ms</span>
    </div>
"#,
            sum.total_duration_ms
        ));
        html.push_str("  </section>\n\n");

        // 3. 统计进度条
        let passed_w = if sum.total_requests > 0 {
            (sum.passed_requests as f64 / sum.total_requests as f64) * 100.0
        } else {
            0.0
        };
        let failed_w = if sum.total_requests > 0 {
            (sum.failed_requests as f64 / sum.total_requests as f64) * 100.0
        } else {
            0.0
        };
        let skipped_w = if sum.total_requests > 0 {
            (sum.skipped_requests as f64 / sum.total_requests as f64) * 100.0
        } else {
            0.0
        };

        html.push_str("  <div class=\"progress-bar-wrap\">\n");
        html.push_str("    <div class=\"progress-bar\">\n");
        if passed_w > 0.0 {
            html.push_str(&format!(
                r#"      <div class="bar-pass" style="width: {:.1}%" title="通过: {}"></div>{}"#,
                passed_w, sum.passed_requests, "\n"
            ));
        }
        if failed_w > 0.0 {
            html.push_str(&format!(
                r#"      <div class="bar-fail" style="width: {:.1}%" title="失败: {}"></div>{}"#,
                failed_w, sum.failed_requests, "\n"
            ));
        }
        if skipped_w > 0.0 {
            html.push_str(&format!(
                r#"      <div class="bar-skip" style="width: {:.1}%" title="跳过: {}"></div>{}"#,
                skipped_w, sum.skipped_requests, "\n"
            ));
        }
        html.push_str("    </div>\n");
        html.push_str("  </div>\n\n");

        // 4. 控制栏（筛选/展开）
        html.push_str("  <div class=\"toolbar\">\n");
        html.push_str("    <span class=\"toolbar-title\">测试套件执行明细</span>\n");
        html.push_str("    <div class=\"toolbar-actions\">\n");
        html.push_str(
            "      <button class=\"btn\" onclick=\"filterRequests('all')\">全部</button>\n",
        );
        html.push_str(
            "      <button class=\"btn btn-danger\" onclick=\"filterRequests('fail')\">仅看失败</button>\n",
        );
        html.push_str(
            "      <button class=\"btn\" onclick=\"toggleAllDetails(true)\">展开所有</button>\n",
        );
        html.push_str(
            "      <button class=\"btn\" onclick=\"toggleAllDetails(false)\">折叠所有</button>\n",
        );
        html.push_str("    </div>\n");
        html.push_str("  </div>\n\n");

        // 5. 文件与请求明细列表
        html.push_str("  <main class=\"suite-list\">\n");

        for file in &context.files {
            let file_path = escape_html(&file.file_path);
            let file_badge_class = if file.failed_requests > 0 {
                "badge-fail"
            } else {
                "badge-pass"
            };

            html.push_str("    <div class=\"file-card\">\n");
            html.push_str("      <div class=\"file-header\">\n");
            html.push_str(&format!(
                r#"        <div class="file-title">📁 <code>{}</code></div>
        <div class="file-meta">
          <span class="badge {}">{} / {} 通过</span>
          <span class="duration">{} ms</span>
        </div>
"#,
                file_path,
                file_badge_class,
                file.passed_requests,
                file.total_requests,
                file.duration_ms
            ));
            html.push_str("      </div>\n");

            html.push_str("      <div class=\"request-list\">\n");

            for req in &file.requests {
                let is_fail = !req.success && !req.skipped;
                let open_attr = if is_fail { " open" } else { "" };
                let req_status_class = if req.skipped {
                    "is-skipped"
                } else if req.success {
                    "is-passed"
                } else {
                    "is-failed"
                };

                let method_class = match req.method.to_uppercase().as_str() {
                    "GET" => "m-get",
                    "POST" => "m-post",
                    "PUT" => "m-put",
                    "DELETE" => "m-delete",
                    "PATCH" => "m-patch",
                    _ => "m-other",
                };

                let status_pill = match req.status {
                    Some(s) if s < 400 => {
                        format!(r#"<span class="status-pill pill-ok">{}</span>"#, s)
                    }
                    Some(s) => format!(r#"<span class="status-pill pill-err">{}</span>"#, s),
                    None if req.skipped => {
                        r#"<span class="status-pill pill-skip">SKIP</span>"#.to_string()
                    }
                    None => r#"<span class="status-pill pill-err">ERR</span>"#.to_string(),
                };

                let name_tag = if let Some(ref n) = req.name {
                    format!(r#"<span class="req-name">{}</span>"#, escape_html(n))
                } else {
                    String::new()
                };

                html.push_str(&format!(
                    r#"        <details class="req-card {}{}">
          <summary class="req-summary">
            <div class="req-main">
              <span class="method-tag {}">{}</span>
              <span class="url">{}</span>
              {}
            </div>
            <div class="req-aside">
              {}
              <span class="req-duration">{} ms</span>
            </div>
          </summary>
          <div class="req-detail">
"#,
                    req_status_class,
                    open_attr,
                    method_class,
                    escape_html(&req.method),
                    escape_html(&req.url),
                    name_tag,
                    status_pill,
                    req.duration_ms
                ));

                // 错误提示
                if let Some(ref err) = req.error {
                    html.push_str(&format!(
                        r#"            <div class="error-banner">❌ <strong>错误:</strong> {}</div>
"#,
                        escape_html(err)
                    ));
                }

                // 断言证据
                if !req.assertions.is_empty() {
                    html.push_str("            <div class=\"assertion-box\">\n");
                    html.push_str("              <div class=\"sub-title\">断言评估证据</div>\n");
                    html.push_str("              <ul class=\"assertion-list\">\n");
                    for a in &req.assertions {
                        Self::render_assertion(&mut html, a);
                    }
                    html.push_str("              </ul>\n");
                    html.push_str("            </div>\n");
                }

                // 告警提示
                if !req.warnings.is_empty() {
                    html.push_str("            <div class=\"warning-box\">\n");
                    html.push_str("              <div class=\"sub-title\">非致命告警</div>\n");
                    html.push_str("              <ul>\n");
                    for w in &req.warnings {
                        html.push_str(&format!("                <li>⚠️ {}</li>\n", escape_html(w)));
                    }
                    html.push_str("              </ul>\n");
                    html.push_str("            </div>\n");
                }

                // 时延瀑布图
                if let Some(ref wf) = req.timing_waterfall {
                    Self::render_waterfall(&mut html, wf);
                }

                // 失败时的响应快照
                if let Some(ref body) = req.body_snippet {
                    html.push_str("            <div class=\"body-snippet-box\">\n");
                    html.push_str(
                        "              <div class=\"sub-title\">响应体片段 (截断 512B)</div>\n",
                    );
                    html.push_str(&format!(
                        "              <pre class=\"code-snippet\"><code>{}</code></pre>\n",
                        escape_html(body)
                    ));
                    html.push_str("            </div>\n");
                }

                html.push_str("          </div>\n");
                html.push_str("        </details>\n");
            }

            html.push_str("      </div>\n");
            html.push_str("    </div>\n");
        }

        html.push_str("  </main>\n");

        // Footer
        html.push_str("  <footer class=\"footer\">\n");
        html.push_str("    <p>Powered by <a href=\"https://github.com/rupost/rupost\" target=\"_blank\">Rupost</a> — The Clean, Document-Driven API Testing Engine.</p>\n");
        html.push_str("  </footer>\n");

        html.push_str("</div>\n");

        // 内联交互脚本 (不到 20 行，零外部框架)
        html.push_str("<script>\n");
        html.push_str(Self::embedded_js());
        html.push_str("</script>\n");

        html.push_str("</body>\n</html>\n");

        Ok(html)
    }
}

impl HtmlReportGenerator {
    fn render_assertion(html: &mut String, a: &ReportAssertionItem) {
        let (icon, item_class) = if a.passed {
            ("✓", "assert-pass")
        } else {
            ("✗", "assert-fail")
        };

        html.push_str(&format!(
            r#"                <li class="{}">
                  <span class="assert-icon">{}</span>
                  <span class="assert-expr">{}</span>
"#,
            item_class,
            icon,
            escape_html(&a.raw)
        ));

        if let Some(ref msg) = a.message {
            html.push_str(&format!(
                r#"                  <div class="assert-msg">{}</div>
"#,
                escape_html(msg)
            ));
        }

        if a.expected.is_some() || a.actual.is_some() {
            html.push_str("                  <div class=\"assert-evidence\">\n");
            if let Some(ref exp) = a.expected {
                html.push_str(&format!(
                    r#"                    <span class="evidence-tag exp">预期: <code>{}</code></span>
"#,
                    escape_html(exp)
                ));
            }
            if let Some(ref act) = a.actual {
                html.push_str(&format!(
                    r#"                    <span class="evidence-tag act">实际: <code>{}</code></span>
"#,
                    escape_html(act)
                ));
            }
            html.push_str("                  </div>\n");
        }

        html.push_str("                </li>\n");
    }

    fn render_waterfall(html: &mut String, wf: &TimingWaterfall) {
        if !wf.total_ms.is_finite() || wf.total_ms <= 0.0 {
            return;
        }

        let dns_pct = (wf.dns_ms / wf.total_ms) * 100.0;
        let tcp_pct = (wf.tcp_ms / wf.total_ms) * 100.0;
        let tls_pct = wf.tls_ms.map(|v| (v / wf.total_ms) * 100.0).unwrap_or(0.0);
        let ttfb_pct = wf.ttfb_ms.map(|v| (v / wf.total_ms) * 100.0).unwrap_or(0.0);
        let transfer_pct = (wf.transfer_ms / wf.total_ms) * 100.0;

        html.push_str("            <div class=\"waterfall-box\">\n");
        html.push_str(&format!(
            "              <div class=\"sub-title\">网络时延瀑布图 (总计: {:.1} ms)</div>\n",
            wf.total_ms
        ));

        html.push_str("              <div class=\"waterfall-bar\">\n");
        if dns_pct > 0.0 {
            html.push_str(&format!(
                r#"                <div class="wf-seg wf-dns" style="width: {:.1}%" title="DNS: {:.1}ms ({:.1}%)"></div>{}"#,
                dns_pct, wf.dns_ms, dns_pct, "\n"
            ));
        }
        if tcp_pct > 0.0 {
            html.push_str(&format!(
                r#"                <div class="wf-seg wf-tcp" style="width: {:.1}%" title="TCP: {:.1}ms ({:.1}%)"></div>{}"#,
                tcp_pct, wf.tcp_ms, tcp_pct, "\n"
            ));
        }
        if tls_pct > 0.0 {
            html.push_str(&format!(
                r#"                <div class="wf-seg wf-tls" style="width: {:.1}%" title="TLS: {:.1}ms ({:.1}%)"></div>{}"#,
                tls_pct, wf.tls_ms.unwrap_or(0.0), tls_pct, "\n"
            ));
        }
        if ttfb_pct > 0.0 {
            html.push_str(&format!(
                r#"                <div class="wf-seg wf-ttfb" style="width: {:.1}%" title="TTFB: {:.1}ms ({:.1}%)"></div>{}"#,
                ttfb_pct, wf.ttfb_ms.unwrap_or(0.0), ttfb_pct, "\n"
            ));
        }
        if transfer_pct > 0.0 {
            html.push_str(&format!(
                r#"                <div class="wf-seg wf-transfer" style="width: {:.1}%" title="Transfer: {:.1}ms ({:.1}%)"></div>{}"#,
                transfer_pct, wf.transfer_ms, transfer_pct, "\n"
            ));
        }
        html.push_str("              </div>\n");

        // 图例与耗时标签
        html.push_str("              <div class=\"wf-legend\">\n");
        if wf.dns_ms > 0.0 {
            html.push_str(&format!(
                r#"                <span class="legend-item"><span class="dot wf-dns"></span>DNS: {:.1}ms</span>{}"#,
                wf.dns_ms, "\n"
            ));
        }
        if wf.tcp_ms > 0.0 {
            html.push_str(&format!(
                r#"                <span class="legend-item"><span class="dot wf-tcp"></span>TCP: {:.1}ms</span>{}"#,
                wf.tcp_ms, "\n"
            ));
        }
        if let Some(tls) = wf.tls_ms
            && tls > 0.0
        {
            html.push_str(&format!(
                r#"                <span class="legend-item"><span class="dot wf-tls"></span>TLS: {:.1}ms</span>{}"#,
                tls, "\n"
            ));
        }
        if let Some(ttfb) = wf.ttfb_ms
            && ttfb > 0.0
        {
            html.push_str(&format!(
                r#"                <span class="legend-item"><span class="dot wf-ttfb"></span>TTFB: {:.1}ms</span>{}"#,
                ttfb, "\n"
            ));
        }
        if wf.transfer_ms > 0.0 {
            html.push_str(&format!(
                r#"                <span class="legend-item"><span class="dot wf-transfer"></span>Transfer: {:.1}ms</span>{}"#,
                wf.transfer_ms, "\n"
            ));
        }
        html.push_str("              </div>\n");

        html.push_str("            </div>\n");
    }

    fn embedded_css() -> &'static str {
        r#"
:root {
  --bg-primary: hsl(220, 16%, 10%);
  --bg-card: hsl(220, 16%, 14%);
  --bg-card-sub: hsl(220, 16%, 18%);
  --border: hsl(220, 14%, 22%);
  --text-main: hsl(220, 14%, 96%);
  --text-muted: hsl(220, 10%, 65%);
  --accent-pass: hsl(150, 70%, 42%);
  --accent-fail: hsl(350, 80%, 55%);
  --accent-skip: hsl(45, 90%, 55%);
  --accent-primary: hsl(210, 90%, 58%);
  --font-sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
  --font-mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
}

* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  background-color: var(--bg-primary);
  color: var(--text-main);
  font-family: var(--font-sans);
  line-height: 1.5;
  padding: 24px;
}
.container { max-width: 1100px; margin: 0 auto; }

/* Header */
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 24px;
  border-bottom: 1px solid var(--border);
  margin-bottom: 24px;
}
.brand .logo {
  font-size: 24px;
  font-weight: 800;
  letter-spacing: -0.5px;
  color: var(--accent-primary);
}
.brand .version {
  font-size: 12px;
  color: var(--text-muted);
  margin-left: 6px;
}
.title-block h1 {
  font-size: 20px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 8px;
}
.title-block .timestamp {
  font-size: 13px;
  color: var(--text-muted);
  text-align: right;
}

/* Badges */
.badge {
  font-size: 12px;
  padding: 3px 8px;
  border-radius: 9999px;
  font-weight: 600;
  display: inline-block;
}
.badge-env { background: hsl(220, 20%, 25%); color: hsl(220, 10%, 85%); }
.badge-pass { background: rgba(34, 197, 94, 0.15); color: var(--accent-pass); }
.badge-fail { background: rgba(239, 68, 68, 0.15); color: var(--accent-fail); }

/* KPI Grid */
.summary-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
  margin-bottom: 20px;
}
.kpi-card {
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
}
.kpi-label { font-size: 13px; color: var(--text-muted); margin-bottom: 6px; }
.kpi-val { font-size: 28px; font-weight: 700; font-family: var(--font-mono); }
.status-pass { color: var(--accent-pass); }
.status-fail { color: var(--accent-fail); }

/* Progress Bar */
.progress-bar-wrap { margin-bottom: 24px; }
.progress-bar {
  display: flex;
  height: 8px;
  border-radius: 4px;
  overflow: hidden;
  background: hsl(220, 16%, 20%);
}
.bar-pass { background: var(--accent-pass); }
.bar-fail { background: var(--accent-fail); }
.bar-skip { background: var(--accent-skip); }

/* Toolbar */
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}
.toolbar-title { font-size: 16px; font-weight: 600; }
.btn {
  background: var(--bg-card);
  color: var(--text-main);
  border: 1px solid var(--border);
  padding: 6px 12px;
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s ease;
}
.btn:hover { background: var(--bg-card-sub); border-color: var(--text-muted); }
.btn-danger { color: var(--accent-fail); }

/* File & Request Cards */
.file-card {
  background: var(--bg-card);
  border: 1px solid var(--border);
  border-radius: 8px;
  margin-bottom: 20px;
  overflow: hidden;
}
.file-header {
  padding: 14px 18px;
  background: var(--bg-card-sub);
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 1px solid var(--border);
}
.file-title code { font-family: var(--font-mono); font-size: 14px; font-weight: 600; }
.file-meta { display: flex; align-items: center; gap: 12px; }
.file-meta .duration { font-size: 13px; color: var(--text-muted); font-family: var(--font-mono); }

/* Request Items */
.req-card {
  border-bottom: 1px solid var(--border);
}
.req-card:last-child { border-bottom: none; }
.req-summary {
  padding: 12px 18px;
  cursor: pointer;
  display: flex;
  justify-content: space-between;
  align-items: center;
  user-select: none;
}
.req-summary:hover { background: rgba(255, 255, 255, 0.02); }
.req-main { display: flex; align-items: center; gap: 10px; font-family: var(--font-mono); font-size: 13px; }
.method-tag {
  font-size: 11px;
  font-weight: 700;
  padding: 2px 6px;
  border-radius: 4px;
}
.m-get { background: rgba(59, 130, 246, 0.2); color: #60a5fa; }
.m-post { background: rgba(16, 185, 129, 0.2); color: #34d399; }
.m-put { background: rgba(245, 158, 11, 0.2); color: #fbbf24; }
.m-delete { background: rgba(239, 68, 68, 0.2); color: #f87171; }
.m-patch { background: rgba(168, 85, 247, 0.2); color: #c084fc; }
.m-other { background: rgba(156, 163, 175, 0.2); color: #9ca3af; }

.req-name {
  font-family: var(--font-sans);
  font-size: 12px;
  color: var(--text-muted);
  background: var(--bg-card-sub);
  padding: 1px 6px;
  border-radius: 4px;
}
.req-aside { display: flex; align-items: center; gap: 10px; font-family: var(--font-mono); font-size: 12px; }
.status-pill {
  padding: 2px 6px;
  border-radius: 4px;
  font-weight: 700;
  font-size: 11px;
}
.pill-ok { background: rgba(34, 197, 94, 0.2); color: var(--accent-pass); }
.pill-err { background: rgba(239, 68, 68, 0.2); color: var(--accent-fail); }
.pill-skip { background: rgba(234, 179, 8, 0.2); color: var(--accent-skip); }
.req-duration { color: var(--text-muted); }

/* Details Inner */
.req-detail {
  padding: 16px 20px;
  background: hsl(220, 16%, 12%);
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.sub-title {
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--text-muted);
  margin-bottom: 8px;
  font-weight: 600;
}
.error-banner {
  background: rgba(239, 68, 68, 0.15);
  border: 1px solid rgba(239, 68, 68, 0.3);
  padding: 10px 14px;
  border-radius: 6px;
  font-size: 13px;
  color: #fca5a5;
}

/* Assertions */
.assertion-list { list-style: none; display: flex; flex-direction: column; gap: 6px; }
.assertion-list li {
  padding: 8px 12px;
  border-radius: 6px;
  background: var(--bg-card);
  border: 1px solid var(--border);
  font-size: 13px;
}
.assert-pass .assert-icon { color: var(--accent-pass); font-weight: 700; margin-right: 6px; }
.assert-fail .assert-icon { color: var(--accent-fail); font-weight: 700; margin-right: 6px; }
.assert-expr { font-family: var(--font-mono); }
.assert-msg { color: #fca5a5; font-size: 12px; margin-top: 4px; margin-left: 18px; }
.assert-evidence { margin-top: 6px; margin-left: 18px; display: flex; gap: 12px; font-size: 12px; }
.evidence-tag code { font-family: var(--font-mono); }
.evidence-tag.exp { color: #93c5fd; }
.evidence-tag.act { color: #fca5a5; }

/* Waterfall */
.waterfall-box {
  background: var(--bg-card);
  padding: 12px 16px;
  border-radius: 6px;
  border: 1px solid var(--border);
}
.waterfall-bar {
  display: flex;
  height: 10px;
  border-radius: 5px;
  overflow: hidden;
  background: hsl(220, 16%, 22%);
  margin-bottom: 10px;
}
.wf-seg { height: 100%; transition: opacity 0.2s; }
.wf-seg:hover { opacity: 0.8; }
.wf-dns { background: hsl(210, 80%, 60%); }
.wf-tcp { background: hsl(180, 70%, 50%); }
.wf-tls { background: hsl(270, 70%, 65%); }
.wf-ttfb { background: hsl(38, 90%, 55%); }
.wf-transfer { background: hsl(320, 70%, 60%); }

.wf-legend { display: flex; flex-wrap: wrap; gap: 14px; font-size: 12px; font-family: var(--font-mono); }
.legend-item { display: flex; align-items: center; gap: 6px; }
.dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }

/* Code Snippet */
.code-snippet {
  background: hsl(220, 20%, 8%);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 10px;
  font-family: var(--font-mono);
  font-size: 12px;
  overflow-x: auto;
  color: hsl(220, 14%, 85%);
}

/* Footer */
.footer {
  text-align: center;
  padding-top: 24px;
  border-top: 1px solid var(--border);
  font-size: 12px;
  color: var(--text-muted);
}
.footer a { color: var(--accent-primary); text-decoration: none; }
.footer a:hover { text-decoration: underline; }
"#
    }

    fn embedded_js() -> &'static str {
        r#"
function toggleAllDetails(open) {
  document.querySelectorAll('.req-card').forEach(d => d.open = open);
}
function filterRequests(mode) {
  document.querySelectorAll('.req-card').forEach(card => {
    if (mode === 'all') {
      card.style.display = '';
    } else if (mode === 'fail') {
      if (card.classList.contains('is-failed')) {
        card.style.display = '';
        card.open = true;
      } else {
        card.style.display = 'none';
      }
    }
  });
}
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::types::{
        ReportAssertionItem, ReportContext, ReportFileEntry, ReportRequestItem, ReportSummary,
        TimingWaterfall,
    };

    #[test]
    fn test_html_report_generator_generates_self_contained_html() {
        let generator = HtmlReportGenerator;

        let wf = TimingWaterfall {
            dns_ms: 2.5,
            tcp_ms: 12.0,
            tls_ms: Some(25.0),
            ttfb_ms: Some(80.0),
            transfer_ms: 15.0,
            total_ms: 134.5,
        };

        let assert_ok = ReportAssertionItem {
            raw: "status == 200".to_string(),
            passed: true,
            message: None,
            expected: None,
            actual: None,
        };

        let assert_err = ReportAssertionItem {
            raw: "body.code == 0".to_string(),
            passed: false,
            message: Some("value mismatch".to_string()),
            expected: Some("0".to_string()),
            actual: Some("40001".to_string()),
        };

        let req1 = ReportRequestItem {
            request_number: 1,
            name: Some("test_auth".to_string()),
            method: "POST".to_string(),
            url: "/api/v1/auth/login".to_string(),
            status: Some(200),
            duration_ms: 135,
            success: true,
            skipped: false,
            error: None,
            assertions: vec![assert_ok],
            warnings: vec![],
            timing_waterfall: Some(wf),
            body_snippet: None,
        };

        let req2 = ReportRequestItem {
            request_number: 2,
            name: Some("test_malformed<script>".to_string()),
            method: "GET".to_string(),
            url: "/api/v1/items?filter=<tag>".to_string(),
            status: Some(400),
            duration_ms: 45,
            success: false,
            skipped: false,
            error: Some("400 Bad Request error".to_string()),
            assertions: vec![assert_err],
            warnings: vec!["token missing".to_string()],
            timing_waterfall: None,
            body_snippet: Some(r#"{"error": "malformed <tag>"}"#.to_string()),
        };

        let file = ReportFileEntry {
            file_path: "examples/test.http".to_string(),
            total_requests: 2,
            passed_requests: 1,
            failed_requests: 1,
            duration_ms: 180,
            requests: vec![req1, req2],
        };

        let context = ReportContext {
            title: "CI Smoke Report".to_string(),
            generated_at: "2026-11-07 00:00:00".to_string(),
            environment: Some("staging".to_string()),
            summary: ReportSummary {
                total_files: 1,
                total_requests: 2,
                passed_requests: 1,
                failed_requests: 1,
                skipped_requests: 0,
                pass_rate: 50.0,
                total_duration_ms: 180,
            },
            files: vec![file],
        };

        let html = generator
            .generate(&context)
            .expect("HTML generation failed");

        // 验证关键 DOM 与转义
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("CI Smoke Report"));
        assert!(html.contains("staging"));
        assert!(html.contains("50.0%"));
        // 验证 XSS 转义防御
        assert!(html.contains("&lt;tag&gt;"));
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
        // 验证时延瀑布图渲染
        assert!(html.contains("网络时延瀑布图 (总计: 134.5 ms)"));
        assert!(html.contains("wf-dns"));
        assert!(html.contains("DNS: 2.5ms"));
        // 验证断言比对证据
        assert!(html.contains("预期: <code>0</code>"));
        assert!(html.contains("实际: <code>40001</code>"));
    }
}
