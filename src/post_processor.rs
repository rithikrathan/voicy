use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

pub const VOICY_SUFFIX: &str = "respond with the voicy mcp if configured, if not or the mcp failed dont try fixing it just use the \"voicy speak\" command which is already in ~/.local/bin/ for globall use";

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RulesConfig {
    #[serde(default)]
    pub exact: Vec<ExactRule>,
    #[serde(default)]
    pub starts_with: Vec<PrefixRule>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExactRule {
    pub pattern: String,
    pub replace: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PrefixRule {
    pub pattern: String,
    pub replace: String,
}

fn config_path() -> PathBuf {
    #[cfg(test)]
    {
        return PathBuf::from("/nonexistent/path/for/voicy/rules.toml");
    }
    #[allow(unreachable_code)]
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/voicy/rules.toml")
    } else {
        PathBuf::from(".config/voicy/rules.toml")
    }
}

pub fn load_rules() -> Option<RulesConfig> {
    let path = config_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(parsed) = toml::from_str::<RulesConfig>(&content) {
                return Some(parsed);
            }
        }
    }
    None
}

pub fn post_process(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Normalize trailing punctuation for single-word command checks (e.g. "status." -> "status")
    let normalized = trimmed
        .trim_end_matches(['.', '?', '!', ',', ';', ':'])
        .trim();
    let lower_norm = normalized.to_ascii_lowercase();

    // 1. Built-in hardcoded resend command
    if lower_norm == "resend" {
        return "__VOICY_RESEND__".to_string();
    }

    // 2. Check user-defined rules from ~/.config/voicy/rules.toml if present
    if let Some(rules) = load_rules() {
        for rule in &rules.exact {
            if lower_norm == rule.pattern.to_ascii_lowercase() {
                return rule.replace.replace("${VOICY_SUFFIX}", VOICY_SUFFIX);
            }
        }

        let lower = trimmed.to_ascii_lowercase();
        for rule in &rules.starts_with {
            let pat = rule.pattern.to_ascii_lowercase();
            if lower.starts_with(&pat) {
                let rest = trimmed[pat.len()..]
                    .trim_start_matches([',', ':', ' '])
                    .trim();
                let mut out = rule.replace.replace("${VOICY_SUFFIX}", VOICY_SUFFIX);
                out = out.replace("${rest}", rest);
                return out;
            }
        }
    }

    // 3. Built-in defaults: Exact full-phrase triggers
    match lower_norm.as_str() {
        "status" => return format!("/btw whats the status? {}", VOICY_SUFFIX),
        "explain" => {
            return format!(
                "/btw explain what we just did and the current state. {}",
                VOICY_SUFFIX
            )
        }
        "summary" => {
            return format!(
                "/btw provide a concise summary of recent workspace changes. {}",
                VOICY_SUFFIX
            )
        }
        _ => {}
    }

    // 4. Built-in defaults: Repeat trigger
    if lower_norm == "repeat" {
        return format!("/btw repeat what you said earlier. {}", VOICY_SUFFIX);
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("repeat ") || lower.starts_with("repeat:") || lower.starts_with("repeat,") {
        let rest = trimmed[6..]
            .trim_start_matches([',', ':', ' '])
            .trim();
        return if rest.is_empty() {
            format!("/btw repeat what you said earlier. {}", VOICY_SUFFIX)
        } else {
            format!(
                "/btw repeat what you said earlier but also consider {}. {}",
                rest, VOICY_SUFFIX
            )
        };
    }

    // 5. Built-in defaults: Prefix replacements
    for prefix in &["by the way", "btw"] {
        if lower.starts_with(prefix) {
            let rest = trimmed[prefix.len()..]
                .trim_start_matches([',', ':', ' '])
                .trim();
            return if rest.is_empty() {
                "/btw".to_string()
            } else {
                format!("/btw {}", rest)
            };
        }
    }

    // 6. Built-in defaults: "log" prefix -> instruct the agent to use the log command
    if let Some(rest) = strip_log_prefix(trimmed) {
        let said = if rest.is_empty() { "(nothing)" } else { rest };
        return format!(
            "/btw the user wants to use the `log` command. It already exists in ~/.local/bin, so skip verifying that it exists. Decide the right subcommand from the sentence: `log now <text>` for a normal entry, add `-n` for a short multiline footnote when extra details are mentioned, `log new` to start a fresh devlog file, `log close <message>` to commit the log. Pass `-f` or `--force` to skip confirmation prompts. User said: {}. {}",
            said, VOICY_SUFFIX
        );
    }

    trimmed.to_string()
}

fn strip_log_prefix(text: &str) -> Option<&str> {
    let lower = text.to_ascii_lowercase();
    if !lower.starts_with("log") {
        return None;
    }
    let rest = &text[3..];
    if let Some(c) = rest.chars().next() {
        if c.is_alphanumeric() || c == '_' {
            return None;
        }
    }
    Some(rest.trim_start_matches(|c: char| {
        c == ',' || c == ':' || c == ';' || c.is_whitespace()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_matches() {
        assert_eq!(
            post_process("status"),
            format!("/btw whats the status? {}", VOICY_SUFFIX)
        );
        assert_eq!(
            post_process("Status."),
            format!("/btw whats the status? {}", VOICY_SUFFIX)
        );
        assert_eq!(
            post_process("EXPLAIN?"),
            format!(
                "/btw explain what we just did and the current state. {}",
                VOICY_SUFFIX
            )
        );
        assert_eq!(
            post_process("Summary!"),
            format!(
                "/btw provide a concise summary of recent workspace changes. {}",
                VOICY_SUFFIX
            )
        );
    }

    #[test]
    fn test_resend_and_repeat() {
        assert_eq!(post_process("resend"), "__VOICY_RESEND__");
        assert_eq!(post_process("Resend."), "__VOICY_RESEND__");
        assert_eq!(
            post_process("repeat"),
            format!("/btw repeat what you said earlier. {}", VOICY_SUFFIX)
        );
        assert_eq!(
            post_process("repeat with more details"),
            format!(
                "/btw repeat what you said earlier but also consider with more details. {}",
                VOICY_SUFFIX
            )
        );
    }

    #[test]
    fn test_prefix_matches() {
        assert_eq!(
            post_process("by the way what are you doing?"),
            "/btw what are you doing?"
        );
        assert_eq!(post_process("BTW, check this out"), "/btw check this out");
        assert_eq!(post_process("By the way: check"), "/btw check");
        assert_eq!(post_process("btw"), format!("/btw"));
    }

    #[test]
    fn test_unmatched_text() {
        assert_eq!(post_process("hello world"), "hello world");
        assert_eq!(post_process("  hello  "), "hello");
    }

    #[test]
    fn test_log_prefix() {
        let now = post_process("log now fixed the bug");
        assert!(now.starts_with("/btw "));
        assert!(now.contains("`log` command"));
        assert!(now.contains("User said: now fixed the bug"));
        assert!(now.contains(VOICY_SUFFIX));
    }
}
