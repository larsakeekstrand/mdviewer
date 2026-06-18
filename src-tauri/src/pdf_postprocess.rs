//! Post-process a WebKit-printed content PDF: re-lay each page onto the target
//! paper at the requested margins (WebKit ignores both CSS @page and
//! NSPrintInfo margins) and stamp page numbers (WebKit has no footer API).
//! macOS-only; built on Core Graphics (CGPDFDocument in, CGPDFContext out).
//!
//! Spike findings — confirmed objc2 0.3 API (against objc2-core-graphics
//! 0.3.2 / objc2-core-foundation 0.3.2 / objc2-core-text 0.3.2):
//!
//! - Read side: `CGPDFDocument::with_url(Some(&CFURL)) -> Option<CFRetained>`;
//!   `CGPDFDocument::number_of_pages(Some(&doc)) -> usize`;
//!   `CGPDFDocument::page(Some(&doc), page_number: usize)` (1-based, `usize`,
//!   NOT i64). Per-page box via `CGPDFPage::box_rect(Some(&page),
//!   CGPDFBox::CropBox)` — the enum is `CGPDFBox` with associated constants
//!   (`MediaBox`/`CropBox`/...), not a `CGPDFPageBoxType`.
//! - Write side: there are NO `CGContext::pdf_with_url` / `begin_pdf_page`
//!   convenience methods in this binding. The PDF context functions are free
//!   functions in `objc2_core_graphics`: `CGPDFContextCreateWithURL(url,
//!   *const CGRect media_box, aux) -> Option<CFRetained<CGContext>>` (unsafe),
//!   `CGPDFContextBeginPage(ctx, page_info)`, `CGPDFContextEndPage(ctx)`,
//!   `CGPDFContextClose(ctx)`. Page drawing/CTM use `CGContext::{save_g_state,
//!   restore_g_state, translate_ctm, scale_ctm, draw_pdf_page}`.
//! - CGRect/CGPoint/CGSize live in `objc2_core_foundation` (CFCGTypes), not
//!   `objc2_core_graphics`.
//! - File URL: `CFURL::with_file_system_path(None, Some(&CFString), style,
//!   is_dir)` with `CFURLPathStyle::CFURLPOSIXPathStyle`. (`CFString::from_str`
//!   builds the path string.)
//! - Footer text: the deprecated CG text API (`CGContext::select_font` /
//!   `show_text_at_point`) is marked `#[deprecated = "No longer supported"]` in
//!   0.3 and would trip `clippy -D warnings`, AND is genuinely unreliable on
//!   modern macOS — so the footer is drawn with Core Text instead (the brief's
//!   sanctioned fallback): a `CFMutableAttributedString` (font +
//!   foreground-color attributes) → `CTLine::with_attributed_string` →
//!   `CTLine::typographic_bounds` for the real advance width (so page-number
//!   centering/right-alignment uses measured, not estimated, width) →
//!   `CTLine::draw(ctx)` after translating the CTM to the baseline origin.
//! - Margin mechanism: WebKit's own ~1-inch margins are baked into the content
//!   PDF's page geometry; we do NOT try to strip them. Instead we treat each
//!   source page as opaque artwork and place it (uniformly scaled to fit the
//!   content width, top-aligned) inside the requested content rect. The
//!   requested margins are therefore honored exactly on the OUTPUT paper; any
//!   WebKit inner margin shows up as extra whitespace within the placed page.
//!   A later task adjusts the print CSS to minimize that inner padding.
//!
//! The module is gated at its `mod` declaration in `lib.rs`
//! (`#[cfg(target_os = "macos")]`); a second inner `#![cfg]` here would be a
//! duplicated-attribute error under `clippy -D warnings`, so it is omitted.
//!
//! The pure geometry/text helpers below are the public interface consumed by
//! `export.rs` (`relayout` + the paper/margin conversions).

use std::path::Path;

use objc2_core_foundation::{
    CFMutableAttributedString, CFRange, CFRetained, CFString, CFType, CFURLPathStyle, CGPoint,
    CGRect, CGSize, CFURL,
};
use objc2_core_graphics::{
    CGColor, CGContext, CGPDFBox, CGPDFContextBeginPage, CGPDFContextClose,
    CGPDFContextCreateWithURL, CGPDFContextEndPage, CGPDFDocument, CGPDFPage,
};
use objc2_core_text::{kCTFontAttributeName, kCTForegroundColorAttributeName, CTFont, CTLine};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarginsPts {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectPts {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Millimetres → PostScript points (72 dpi).
pub fn mm_to_points(mm: f64) -> f64 {
    mm * 72.0 / 25.4
}

/// Portrait paper size in points. Unknown names fall back to A4.
pub fn paper_points(paper: &str) -> (f64, f64) {
    match paper {
        "letter" => (612.0, 792.0),
        "legal" => (612.0, 1008.0),
        _ => (595.28, 841.89), // a4
    }
}

/// Portrait paper dimensions from `paper_points`, swapped to landscape when
/// requested. Width/height are in PostScript points.
pub fn oriented(paper: &str, landscape: bool) -> (f64, f64) {
    let (w, h) = paper_points(paper);
    if landscape {
        (h, w)
    } else {
        (w, h)
    }
}

/// The drawable box inside the page after subtracting margins. Origin is the
/// PDF coordinate system (bottom-left).
pub fn content_rect(paper_w: f64, paper_h: f64, m: &MarginsPts) -> RectPts {
    RectPts {
        x: m.left,
        y: m.bottom,
        w: (paper_w - m.left - m.right).max(1.0),
        h: (paper_h - m.top - m.bottom).max(1.0),
    }
}

/// Uniform scale so a `src_w`-wide source fits the content width, never
/// upscaling (so a normal page is reproduced 1:1; only over-wide content
/// shrinks). `src_h` is accepted for symmetry/future height-fit needs.
pub fn fit_scale(src_w: f64, _src_h: f64, dst: &RectPts) -> f64 {
    if src_w <= 0.0 {
        return 1.0;
    }
    (dst.w / src_w).min(1.0)
}

/// The footer text for a 0-based page index, or None when numbering is off.
pub fn page_number_text(page_index: usize, total: usize, mode: &str) -> Option<String> {
    match mode {
        "bottom-center" | "bottom-right" => Some(format!("{} / {}", page_index + 1, total)),
        _ => None,
    }
}

/// X origin (points, from page left) for footer text of width `text_width`
/// within `content`, or None when numbering is off.
pub fn page_number_x(mode: &str, content: &RectPts, text_width: f64) -> Option<f64> {
    match mode {
        "bottom-center" => Some(content.x + (content.w - text_width) / 2.0),
        "bottom-right" => Some(content.x + content.w - text_width),
        _ => None,
    }
}

fn cg_rect(x: f64, y: f64, w: f64, h: f64) -> CGRect {
    CGRect {
        origin: CGPoint { x, y },
        size: CGSize {
            width: w,
            height: h,
        },
    }
}

fn file_url(p: &Path) -> Option<CFRetained<CFURL>> {
    let s = p.to_str()?;
    let cf = CFString::from_str(s);
    CFURL::with_file_system_path(None, Some(&cf), CFURLPathStyle::CFURLPOSIXPathStyle, false)
}

/// Re-lay `src` onto `dst` at `paper_w`×`paper_h` (points) with `margins`, and
/// stamp page numbers per `page_numbers`. Returns Err with context on any
/// Core Graphics failure.
pub fn relayout(
    src: &Path,
    dst: &Path,
    paper_w: f64,
    paper_h: f64,
    margins: &MarginsPts,
    page_numbers: &str,
) -> Result<(), String> {
    // SAFETY: all pointers below come from CG constructors checked for null;
    // every page begin/end is balanced and the context is closed before return.
    unsafe {
        let src_url = file_url(src).ok_or("bad source path")?;
        let doc = CGPDFDocument::with_url(Some(&src_url))
            .ok_or_else(|| format!("could not open content PDF: {}", src.display()))?;
        let total = CGPDFDocument::number_of_pages(Some(&doc));
        if total == 0 {
            return Err("content PDF has no pages".to_string());
        }

        let dst_url = file_url(dst).ok_or("bad output path")?;
        let media = cg_rect(0.0, 0.0, paper_w, paper_h);
        let ctx = CGPDFContextCreateWithURL(Some(&dst_url), &media, None)
            .ok_or("could not create output PDF context")?;

        let content = content_rect(paper_w, paper_h, margins);

        for i in 1..=total {
            let page = CGPDFDocument::page(Some(&doc), i)
                .ok_or_else(|| format!("missing source page {i}"))?;
            let src_box = CGPDFPage::box_rect(Some(&page), CGPDFBox::CropBox);
            let (sw, sh) = (src_box.size.width, src_box.size.height);

            CGPDFContextBeginPage(Some(&ctx), None);
            CGContext::save_g_state(Some(&ctx));

            let scale = fit_scale(sw, sh, &content);
            let scaled_h = sh * scale;
            // Top-align inside the content box.
            let ty = content.y + content.h - scaled_h;
            CGContext::translate_ctm(Some(&ctx), content.x, ty);
            CGContext::scale_ctm(Some(&ctx), scale, scale);
            // Shift so the source crop-box origin maps to (0,0).
            CGContext::translate_ctm(Some(&ctx), -src_box.origin.x, -src_box.origin.y);
            CGContext::draw_pdf_page(Some(&ctx), Some(&page));

            CGContext::restore_g_state(Some(&ctx));

            if let Some(text) = page_number_text(i - 1, total, page_numbers) {
                draw_footer(&ctx, &content, margins, &text, page_numbers);
            }

            CGPDFContextEndPage(Some(&ctx));
        }

        CGPDFContextClose(Some(&ctx));
    }
    Ok(())
}

/// Draw `text` as a centered/right footer baseline `margins.bottom*0.5` up from
/// the page bottom, in 9pt Helvetica grey, via Core Text (the CG text API is
/// deprecated/unsupported in objc2 0.3). The footer x-origin uses the line's
/// measured advance width so centering and right-alignment are exact.
///
/// # Safety
/// `ctx` must be a live PDF context with a page begun.
unsafe fn draw_footer(
    ctx: &CGContext,
    content: &RectPts,
    margins: &MarginsPts,
    text: &str,
    mode: &str,
) {
    let font_size = 9.0_f64;
    let helvetica = CFString::from_str("Helvetica");
    let font = CTFont::with_name(&helvetica, font_size, std::ptr::null());
    let grey = CGColor::new_generic_gray(0.4, 1.0);

    let cf_text = CFString::from_str(text);
    let Some(attr) = CFMutableAttributedString::new(None, 0) else {
        return;
    };
    let len = cf_text.length();
    CFMutableAttributedString::replace_string(
        Some(&attr),
        CFRange {
            location: 0,
            length: 0,
        },
        Some(&cf_text),
    );
    let full = CFRange {
        location: 0,
        length: len,
    };
    let font_cf: &CFType = font.as_ref();
    CFMutableAttributedString::set_attribute(
        Some(&attr),
        full,
        Some(kCTFontAttributeName),
        Some(font_cf),
    );
    let color_cf: &CFType = grey.as_ref();
    CFMutableAttributedString::set_attribute(
        Some(&attr),
        full,
        Some(kCTForegroundColorAttributeName),
        Some(color_cf),
    );

    let line = CTLine::with_attributed_string(&attr);
    let width = CTLine::typographic_bounds(
        &line,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );

    let Some(x) = page_number_x(mode, content, width) else {
        return;
    };
    let y = (margins.bottom * 0.5).max(8.0);

    CGContext::save_g_state(Some(ctx));
    CGContext::translate_ctm(Some(ctx), x, y);
    CTLine::draw(&line, ctx);
    CGContext::restore_g_state(Some(ctx));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_to_points_converts() {
        assert!((mm_to_points(25.4) - 72.0).abs() < 1e-9);
        assert!((mm_to_points(0.0)).abs() < 1e-9);
    }

    #[test]
    fn paper_points_known_and_fallback() {
        assert_eq!(paper_points("letter"), (612.0, 792.0));
        assert_eq!(paper_points("legal"), (612.0, 1008.0));
        assert_eq!(paper_points("a4"), (595.28, 841.89));
        assert_eq!(paper_points("garbage"), (595.28, 841.89));
    }

    #[test]
    fn oriented_swaps_dimensions_for_landscape() {
        assert_eq!(oriented("a4", false), (595.28, 841.89));
        assert_eq!(oriented("a4", true), (841.89, 595.28));
        assert_eq!(oriented("letter", false), (612.0, 792.0));
        assert_eq!(oriented("letter", true), (792.0, 612.0));
        // Unknown paper still falls back to A4, swapped when landscape.
        assert_eq!(oriented("garbage", true), (841.89, 595.28));
    }

    #[test]
    fn content_rect_subtracts_margins() {
        let r = content_rect(
            600.0,
            800.0,
            &MarginsPts {
                top: 50.0,
                right: 40.0,
                bottom: 60.0,
                left: 30.0,
            },
        );
        assert_eq!(
            r,
            RectPts {
                x: 30.0,
                y: 60.0,
                w: 530.0,
                h: 690.0
            }
        );
    }

    #[test]
    fn fit_scale_never_upscales() {
        let dst = RectPts {
            x: 0.0,
            y: 0.0,
            w: 500.0,
            h: 700.0,
        };
        assert!((fit_scale(1000.0, 1400.0, &dst) - 0.5).abs() < 1e-9);
        assert!((fit_scale(400.0, 560.0, &dst) - 1.0).abs() < 1e-9); // would-be 1.25 clamped
    }

    #[test]
    fn page_number_text_modes() {
        assert_eq!(
            page_number_text(0, 3, "bottom-center"),
            Some("1 / 3".to_string())
        );
        assert_eq!(
            page_number_text(2, 3, "bottom-right"),
            Some("3 / 3".to_string())
        );
        assert_eq!(page_number_text(0, 3, "none"), None);
    }

    #[test]
    fn page_number_x_positions() {
        let c = RectPts {
            x: 30.0,
            y: 60.0,
            w: 500.0,
            h: 700.0,
        };
        assert_eq!(page_number_x("none", &c, 40.0), None);
        assert_eq!(page_number_x("bottom-right", &c, 40.0), Some(490.0));
        assert_eq!(page_number_x("bottom-center", &c, 40.0), Some(260.0));
    }
}
