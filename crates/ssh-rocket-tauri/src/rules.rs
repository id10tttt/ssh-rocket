use ssh_rocket_core::{
    parse_rule_set, parse_shadowrocket_rules, RuleImportResult,
};
use std::process::Command as StdCommand;

pub const MAX_RULE_SOURCE_SIZE: usize = 16 * 1024 * 1024;

pub fn import_rule_source(url: &str) -> Result<RuleImportResult, String> {
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 协议的规则订阅链接".into());
    }
    let content = download_rule_text(url)?;
    let mut result = parse_shadowrocket_rules(&content);
    let rule_sets = result.rule_sets.clone();
    for reference in rule_sets.into_iter().take(8) {
        match download_rule_text(&reference.url) {
            Ok(content) => result.merge(parse_rule_set(&content, reference.action)),
            Err(error) => {
                result.ignored_count += 1;
                result.warnings.push(format!("子规则集跳过: {error}"));
            }
        }
    }
    if result.rule_sets.len() > 8 {
        result.ignored_count += result.rule_sets.len() - 8;
        result.warnings.push("部分超出数量限制的子规则集已被跳过".into());
    }
    if result.rule_count() == 0 {
        return Err("规则源不包含任何支持的有效规则".into());
    }
    Ok(result)
}

pub fn download_rule_text(url: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 协议的规则订阅链接".into());
    }
    let output = StdCommand::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "120",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            &MAX_RULE_SOURCE_SIZE.to_string(),
            url,
        ])
        .output()
        .map_err(|error| format!("curl 命令启动失败: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() {
            format!("curl 进程退出码: {}", output.status)
        } else {
            error
        });
    }
    if output.stdout.len() > MAX_RULE_SOURCE_SIZE {
        return Err("规则源文件大小超过 16 MB 限制".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "规则内容非有效 UTF-8 编码".into())
}
