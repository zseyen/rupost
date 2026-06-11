pub mod strategy;

use std::fs;
use std::path::Path;
use strategy::{SseTemplate, TemplateStrategy};

/// 获取所有可用的模板策略列表
pub fn get_templates() -> Vec<Box<dyn TemplateStrategy>> {
    vec![Box::new(SseTemplate)]
}

/// 执行模板生成命令的机制层主逻辑
pub fn run_template(r#type: &str, output: &str, force: bool, list: bool) -> crate::Result<()> {
    let templates = get_templates();

    // 1. 处理 --list 选项
    if list || r#type == "list" {
        println!("Available templates:");
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
                "Unsupported template type '{}'. Use 'rupost template --list' to view available templates.",
                r#type
            ))
        })?;

    // 3. 计算路径：.http 模板路径和 .env.example 路径
    let http_path = Path::new(output);
    let parent_dir = http_path.parent().unwrap_or_else(|| Path::new(""));
    let env_path = parent_dir.join(".env.example");

    // 4. 安全覆盖拦截预检
    if !force {
        if http_path.exists() {
            return Err(crate::RupostError::Other(format!(
                "File already exists: {}. Use --force to overwrite.",
                output
            )));
        }
        if let Some(_env_content) = strategy.env_content() {
            if env_path.exists() {
                return Err(crate::RupostError::Other(format!(
                    "File already exists: {}. Use --force to overwrite.",
                    env_path.display()
                )));
            }
        }
    }

    // 5. 自动创建父级目录（如果存在父目录且尚未创建）
    if !parent_dir.as_os_str().is_empty() && !parent_dir.exists() {
        fs::create_dir_all(parent_dir)?;
    }

    // 6. 根据文件扩展名选取模板内容
    let ext = http_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let is_markdown = ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown");

    let template_content = if is_markdown {
        strategy.markdown_content()
    } else {
        strategy.http_content()
    };

    // 7. 执行物理写入
    fs::write(http_path, template_content)?;
    println!("Generated {} test template file: {}", strategy.name(), output);

    if let Some(env_content) = strategy.env_content() {
        fs::write(&env_path, env_content)?;
        println!("Generated accompanying config file: {}", env_path.display());
    }

    Ok(())
}
