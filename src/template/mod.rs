pub mod strategy;

use colored::Colorize;
use std::fs;
use std::path::Path;
use strategy::{ConfigTemplate, LlmTemplate, SseTemplate, TemplateStrategy};

/// 获取所有可用的模板策略列表
pub fn get_templates() -> Vec<Box<dyn TemplateStrategy>> {
    vec![
        Box::new(ConfigTemplate),
        Box::new(SseTemplate),
        Box::new(LlmTemplate),
    ]
}


/// 执行模板生成命令的机制层主逻辑
pub fn run_template(
    r#type: &str,
    output: Option<&str>,
    force: bool,
    list: bool,
) -> crate::Result<()> {
    let templates = get_templates();

    // 1. 处理 --list 选项
    if list || r#type == "list" {
        println!("Available templates for initialization:");
        for t in templates {
            println!("  - {:<10} : {}", t.name(), t.description());
        }
        return Ok(());
    }

    // 2. 匹配具体模板策略
    let strategy = templates
        .into_iter()
        .find(|t| t.name() == r#type)
        .ok_or_else(|| {
            crate::RupostError::Other(format!(
                "Unsupported template type '{}'. Use 'rupost init --list' to view available templates.",
                r#type
            ))
        })?;

    // 3. 计算默认输出文件名
    let resolved_output = match output {
        Some(path) => path.to_string(),
        None => {
            if strategy.name() == "config" {
                "rupost.toml".to_string()
            } else {
                format!("{}_template.http", strategy.name())
            }
        }
    };

    let http_path = Path::new(&resolved_output);
    let parent_dir = http_path.parent().unwrap_or_else(|| Path::new(""));
    let env_path = parent_dir.join(".env.example");

    // 4. 安全覆盖预检
    let http_exists = http_path.exists();
    let env_exists = strategy.env_content().is_some() && env_path.exists();

    if force {
        if http_exists {
            println!(
                "{} Warning: File already exists: {}. Overwriting due to --force.",
                "[!]".bold().yellow(),
                resolved_output
            );
        }
        if env_exists {
            println!(
                "{} Warning: File already exists: {}. Overwriting due to --force.",
                "[!]".bold().yellow(),
                env_path.display()
            );
        }
    } else {
        if http_exists {
            return Err(crate::RupostError::Other(format!(
                "File already exists: {}. Use --force to overwrite.",
                resolved_output
            )));
        }
        if env_exists {
            return Err(crate::RupostError::Other(format!(
                "File already exists: {}. Use --force to overwrite.",
                env_path.display()
            )));
        }
    }

    // 5. 自动创建父级目录（如果存在父目录且尚未创建）
    if !parent_dir.as_os_str().is_empty() && !parent_dir.exists() {
        fs::create_dir_all(parent_dir)?;
    }

    // 6. 根据文件扩展名选取模板内容
    let ext = http_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let is_markdown = ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown");

    let template_content = if is_markdown {
        strategy.markdown_content()
    } else {
        strategy.http_content()
    };

    // 7. 执行物理写入
    fs::write(http_path, template_content)?;
    println!(
        "Successfully initialized {}: {}",
        strategy.name(),
        resolved_output
    );

    if let Some(env_content) = strategy.env_content() {
        fs::write(&env_path, env_content)?;
        println!("Generated accompanying config file: {}", env_path.display());
    }

    Ok(())
}
