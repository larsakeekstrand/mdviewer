use std::sync::LazyLock;

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

pub fn render_markdown(source: &str, theme: &str) -> String {
    let opts = build_options();
    let adapter: &SyntectAdapter = match theme {
        "dark" => &DARK_ADAPTER,
        _ => &LIGHT_ADAPTER,
    };
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(adapter);
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
