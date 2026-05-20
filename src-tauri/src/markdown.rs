use std::fmt;
use std::sync::LazyLock;

use comrak::adapters::CodefenceRendererAdapter;
use comrak::nodes::Sourcepos;
use comrak::options::Plugins;
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{markdown_to_html_with_plugins, Options};

static LIGHT_ADAPTER: LazyLock<SyntectAdapter> =
    LazyLock::new(|| SyntectAdapter::new(Some("InspiredGitHub")));

static DARK_ADAPTER: LazyLock<SyntectAdapter> =
    LazyLock::new(|| SyntectAdapter::new(Some("base16-ocean.dark")));

pub fn prewarm() {
    let _ = render_markdown("# warmup\n\n```rust\nfn main() {}\n```\n", "light");
    let _ = render_markdown("# warmup\n\n```rust\nfn main() {}\n```\n", "dark");
}

fn build_options() -> Options<'static> {
    let mut opts = Options::default();
    // GitHub-flavored markdown
    opts.extension.table = true;
    opts.extension.tasklist = true;
    opts.extension.strikethrough = true;
    opts.extension.autolink = true;
    opts.extension.footnotes = true;
    opts.extension.tagfilter = true;
    opts.extension.header_id_prefix = Some("md-h-".to_string());
    // Source positions enable scroll-anchor preservation across live reload.
    opts.render.sourcepos = true;
    // Block raw inline HTML in markdown.
    opts.render.r#unsafe = false;
    opts
}

struct MermaidRenderer;

impl CodefenceRendererAdapter for MermaidRenderer {
    fn write(
        &self,
        output: &mut dyn fmt::Write,
        _lang: &str,
        _meta: &str,
        code: &str,
        sourcepos: Option<Sourcepos>,
    ) -> fmt::Result {
        output.write_str("<pre class=\"mermaid\"")?;
        if let Some(sp) = sourcepos {
            output.write_str(" data-sourcepos=\"")?;
            write!(output, "{sp}")?;
            output.write_str("\"")?;
        }
        output.write_str(">")?;
        write_html_escaped(output, code)?;
        output.write_str("</pre>\n")
    }
}

fn write_html_escaped(output: &mut dyn fmt::Write, s: &str) -> fmt::Result {
    for c in s.chars() {
        match c {
            '&' => output.write_str("&amp;")?,
            '<' => output.write_str("&lt;")?,
            '>' => output.write_str("&gt;")?,
            _ => output.write_char(c)?,
        }
    }
    Ok(())
}

pub fn render_markdown(source: &str, theme: &str) -> String {
    let opts = build_options();
    let adapter: &SyntectAdapter = match theme {
        "dark" => &DARK_ADAPTER,
        _ => &LIGHT_ADAPTER,
    };
    let mermaid = MermaidRenderer;
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(adapter);
    plugins
        .render
        .codefence_renderers
        .insert("mermaid".to_string(), &mermaid);
    markdown_to_html_with_plugins(source, &opts, &plugins)
}

pub fn render_plain(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<pre class=\"plain-text\"><code>{escaped}</code></pre>")
}

pub fn is_markdown_path(path: &std::path::Path) -> bool {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => matches!(
            ext.to_ascii_lowercase().as_str(),
            "md" | "markdown" | "mdown" | "mkd" | "mkdn"
        ),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_fence_renders_as_mermaid_pre() {
        let html = render_markdown("```mermaid\ngraph TD;\n  A-->B;\n```\n", "light");
        assert!(
            html.contains("<pre class=\"mermaid\""),
            "expected a mermaid pre, got: {html}"
        );
        assert!(
            html.contains("A--&gt;B"),
            "expected escaped source, got: {html}"
        );
        assert!(
            !html.contains("background-color"),
            "mermaid block should bypass syntect, got: {html}"
        );
        assert!(!html.contains("language-mermaid"), "got: {html}");
    }

    #[test]
    fn non_mermaid_fence_still_highlighted() {
        let html = render_markdown("```rust\nfn main() {}\n```\n", "light");
        // syntect emits an inline background-color on the <pre>.
        assert!(
            html.contains("background-color"),
            "rust block should still be syntect-highlighted, got: {html}"
        );
    }
}
