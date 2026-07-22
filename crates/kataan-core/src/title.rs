use std::path::Path;

/// Builds a human-readable title from the final segment of a canonical id/path.
pub fn title_from_id(id: &str) -> String {
    title_case_slug(id.rsplit('/').next().unwrap_or(id))
}

/// Builds a human-readable title from a filesystem path's final component.
pub fn title_from_path(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(title_case_slug)
        .unwrap_or_else(|| title_case_slug(fallback))
}

/// Converts slug-like text such as `company-x` into `Company X`.
pub fn title_case_slug(value: &str) -> String {
    value
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_first(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_slug_like_values() {
        assert_eq!(title_case_slug("type-definition"), "Type Definition");
        assert_eq!(title_from_id("projects/company-x"), "Company X");
        assert_eq!(
            title_from_path(Path::new("notes/session_store"), "folder"),
            "Session Store"
        );
    }
}
