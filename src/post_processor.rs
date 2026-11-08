const VOICY_SUFFIX: &str = "respond with the voicy mcp if configured, if not or the mcp failed dont try fixing it just use the \"voicy speak\" command which is already in ~/.local/bin/ for globall use";

pub fn post_process(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Normalize trailing punctuation for single-word command checks (e.g. "status." -> "status")
    let normalized = trimmed.trim_end_matches(['.', '?', '!', ',', ';', ':']).trim();

    // Exact full-phrase triggers
    match normalized.to_ascii_lowercase().as_str() {
        "status" => return format!("/btw whats the status? {}", VOICY_SUFFIX),
        "explain" => return format!("/btw explain what we just did and the current state. {}", VOICY_SUFFIX),
        "summary" => return format!("/btw provide a concise summary of recent workspace changes. {}", VOICY_SUFFIX),
        _ => {}
    }

    // Prefix replacements (case-insensitive)
    let lower = trimmed.to_ascii_lowercase();
    for prefix in &["by the way", "btw"] {
        if lower.starts_with(prefix) {
            let rest = trimmed[prefix.len()..].trim_start_matches([',', ':', ' ']).trim();
            return if rest.is_empty() {
                "/btw".to_string()
            } else {
                format!("/btw {}", rest)
            };
        }
    }

    trimmed.to_string()
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
            format!("/btw explain what we just did and the current state. {}", VOICY_SUFFIX)
        );
        assert_eq!(
            post_process("Summary!"),
            format!("/btw provide a concise summary of recent workspace changes. {}", VOICY_SUFFIX)
        );
    }

    #[test]
    fn test_prefix_matches() {
        assert_eq!(
            post_process("by the way what are you doing?"),
            "/btw what are you doing?"
        );
        assert_eq!(
            post_process("BTW, check this out"),
            "/btw check this out"
        );
        assert_eq!(
            post_process("By the way: check"),
            "/btw check"
        );
        assert_eq!(
            post_process("btw"),
            format!("/btw")
        );
    }

    #[test]
    fn test_unmatched_text() {
        assert_eq!(post_process("hello world"), "hello world");
        assert_eq!(post_process("  hello  "), "hello");
    }
}
