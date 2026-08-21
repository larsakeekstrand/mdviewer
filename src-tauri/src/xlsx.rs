use crate::code::escape_html;
use std::path::Path;

const SHEET_EXTS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    DateTime(f64),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Debug, Clone)]
pub struct Caps {
    pub max_file_bytes: usize,
    pub max_rows_per_sheet: usize,
    pub max_total_cells: usize,
    pub max_cell_chars: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            max_file_bytes: 32 * 1024 * 1024,
            max_rows_per_sheet: 5_000,
            max_total_cells: 200_000,
            // Excel's own native cell text limit.
            max_cell_chars: 32_767,
        }
    }
}

pub fn is_spreadsheet_path(p: &Path) -> bool {
    match p.extension().and_then(|e| e.to_str()) {
        Some(ext) => SHEET_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// Excel day serial → ISO date (with a time component only when the serial
/// carries a fractional day). Excel's 1900 system contains a phantom
/// 1900-02-29 at serial 60, so serials at or below 59 sit one day ahead of the
/// real calendar and take a different epoch offset.
pub fn excel_serial_to_iso(serial: f64) -> String {
    let days = serial.trunc() as i64;
    let epoch_offset = if days <= 59 { 25_568 } else { 25_569 };
    let (y, m, d) = civil_from_days(days - epoch_offset);
    let date = format!("{y:04}-{m:02}-{d:02}");

    let frac = serial - serial.trunc();
    if frac <= f64::EPSILON {
        return date;
    }
    let total = (frac * 86_400.0).round() as i64;
    format!(
        "{date} {:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// Howard Hinnant's `civil_from_days`, verbatim: days since 1970-01-01 → (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn cell_to_string(c: &Cell) -> String {
    match c {
        Cell::Empty => String::new(),
        Cell::Text(s) => s.clone(),
        Cell::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Cell::Error(e) => e.clone(),
        Cell::DateTime(v) => excel_serial_to_iso(*v),
        // Rust's f64 Display already renders 3.0 as "3" and never uses
        // scientific notation, which is exactly the spreadsheet convention.
        Cell::Number(n) => format!("{n}"),
    }
}

fn truncation_notice(what: &str) -> String {
    format!(
        "<p class=\"sheet-truncated\">{} truncated for preview.</p>",
        escape_html(what)
    )
}

pub fn render_sheets(sheets: &[Sheet], caps: &Caps) -> String {
    if sheets.is_empty() {
        return "<p class=\"sheet-empty\">This workbook has no sheets.</p>".to_string();
    }
    let mut out = String::new();
    let mut budget = caps.max_total_cells;

    for sheet in sheets {
        out.push_str(&format!("<h2>{}</h2>", escape_html(&sheet.name)));
        if budget == 0 {
            out.push_str(&truncation_notice("Remaining sheets"));
            break;
        }
        if sheet.rows.is_empty() {
            out.push_str("<p class=\"sheet-empty\">Empty sheet.</p>");
            continue;
        }

        let width = sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut truncated = sheet.rows.len() > caps.max_rows_per_sheet;
        let limit = sheet.rows.len().min(caps.max_rows_per_sheet);
        let mut tbody_open = false;

        out.push_str("<table>");
        for (i, row) in sheet.rows.iter().take(limit).enumerate() {
            if budget < width {
                truncated = true;
                break;
            }
            budget -= width;
            let tag = if i == 0 { "th" } else { "td" };
            if i == 0 {
                out.push_str("<thead>");
            } else if i == 1 {
                out.push_str("<tbody>");
                tbody_open = true;
            }
            out.push_str("<tr>");
            for col in 0..width {
                let text = row.get(col).map(cell_to_string).unwrap_or_default();
                out.push_str(&format!("<{tag}>{}</{tag}>", escape_html(&text)));
            }
            out.push_str("</tr>");
            if i == 0 {
                out.push_str("</thead>");
            }
        }
        if tbody_open {
            out.push_str("</tbody>");
        }
        out.push_str("</table>");
        if truncated {
            out.push_str(&truncation_notice("Sheet"));
        }
    }
    out
}

use calamine::{Data, Reader};
use std::io::Cursor;

// Excel/ODS can label an attacker-controlled string as a "date" cell whose
// text failed strict ISO 8601 parsing (DateTimeIso/DurationIso carry the raw
// source text in that case), so those two variants get the same text cap as
// Data::String rather than being assumed short.
fn from_calamine(d: &Data, max_chars: usize) -> Cell {
    match d {
        Data::Empty => Cell::Empty,
        Data::String(s) => Cell::Text(truncate_chars(s, max_chars)),
        Data::Float(f) => Cell::Number(*f),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Bool(b) => Cell::Bool(*b),
        Data::Error(e) => Cell::Error(format!("{e:?}")),
        Data::DateTime(dt) => Cell::DateTime(dt.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(truncate_chars(s, max_chars)),
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => s[..byte_idx].to_string(),
        None => s.to_string(),
    }
}

pub fn render_workbook(bytes: &[u8], caps: &Caps) -> Result<String, String> {
    if bytes.len() > caps.max_file_bytes {
        return Err(format!(
            "spreadsheet is too large to preview ({} MB; limit {} MB)",
            bytes.len() / (1024 * 1024),
            caps.max_file_bytes / (1024 * 1024)
        ));
    }

    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("cannot read spreadsheet: {e}"))?;

    let names = wb.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    // Enforced here, not only inside render_sheets: without this, every
    // sheet is read and fully materialized (worksheet_range) before any cap
    // acts on it, which lets a small compressed file expand hugely in
    // memory before being truncated for display.
    let mut remaining = caps.max_total_cells;
    let mut dropped_sheets = false;

    for name in names {
        if remaining == 0 {
            dropped_sheets = true;
            break;
        }
        let range = wb
            .worksheet_range(&name)
            .map_err(|e| format!("cannot read spreadsheet sheet '{name}': {e}"))?;

        let mut rows = Vec::new();
        for r in range.rows().take(caps.max_rows_per_sheet) {
            if remaining == 0 {
                break;
            }
            let row: Vec<Cell> = r
                .iter()
                .map(|d| from_calamine(d, caps.max_cell_chars))
                .collect();
            remaining = remaining.saturating_sub(row.len());
            rows.push(row);
        }
        sheets.push(Sheet { name, rows });
    }

    let mut html = render_sheets(&sheets, caps);
    if dropped_sheets {
        html.push_str(&truncation_notice("Remaining sheets"));
    }
    Ok(html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognizes_spreadsheet_extensions_case_insensitively() {
        for p in ["a.xlsx", "a.XLSM", "a.xlsb", "a.xls", "a.ods"] {
            assert!(is_spreadsheet_path(Path::new(p)), "{p}");
        }
        for p in ["a.md", "a.pdf", "a.xlsxx", "a", "a.csv"] {
            assert!(!is_spreadsheet_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn converts_excel_serials_to_iso_dates() {
        // Serial 1 is 1900-01-01; Excel's phantom 1900-02-29 sits at serial 60,
        // so serials at or below 59 need a one-day shift and later ones do not.
        assert_eq!(excel_serial_to_iso(1.0), "1900-01-01");
        assert_eq!(excel_serial_to_iso(59.0), "1900-02-28");
        assert_eq!(excel_serial_to_iso(61.0), "1900-03-01");
        assert_eq!(excel_serial_to_iso(25569.0), "1970-01-01");
        assert_eq!(excel_serial_to_iso(45000.0), "2023-03-15");
    }

    #[test]
    fn appends_a_time_component_only_when_the_serial_has_one() {
        assert_eq!(excel_serial_to_iso(45000.0), "2023-03-15");
        assert_eq!(excel_serial_to_iso(45000.5), "2023-03-15 12:00:00");
        assert_eq!(excel_serial_to_iso(45000.25), "2023-03-15 06:00:00");
    }

    #[test]
    fn formats_numbers_without_trailing_noise() {
        assert_eq!(cell_to_string(&Cell::Number(3.0)), "3");
        assert_eq!(cell_to_string(&Cell::Number(3.5)), "3.5");
        assert_eq!(cell_to_string(&Cell::Number(-0.25)), "-0.25");
        assert_eq!(cell_to_string(&Cell::Empty), "");
        assert_eq!(cell_to_string(&Cell::Bool(true)), "TRUE");
        assert_eq!(cell_to_string(&Cell::Text("hi".into())), "hi");
        assert_eq!(cell_to_string(&Cell::Error("#DIV/0!".into())), "#DIV/0!");
    }

    fn sheet(name: &str, rows: &[&[&str]]) -> Sheet {
        Sheet {
            name: name.to_string(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|c| Cell::Text(c.to_string())).collect())
                .collect(),
        }
    }

    #[test]
    fn renders_first_row_as_a_header_and_the_rest_as_body() {
        let html = render_sheets(
            &[sheet("Sheet1", &[&["Item", "Qty"], &["Bolt", "120"]])],
            &Caps::default(),
        );
        assert!(html.contains("<h2>Sheet1</h2>"), "{html}");
        assert!(
            html.contains("<thead><tr><th>Item</th><th>Qty</th></tr></thead>"),
            "{html}"
        );
        assert!(
            html.contains("<tbody><tr><td>Bolt</td><td>120</td></tr></tbody>"),
            "{html}"
        );
    }

    #[test]
    fn renders_every_sheet_in_workbook_order() {
        let html = render_sheets(
            &[sheet("First", &[&["a"]]), sheet("Second", &[&["b"]])],
            &Caps::default(),
        );
        let first = html.find("First").unwrap();
        let second = html.find("Second").unwrap();
        assert!(first < second, "sheets out of order: {html}");
    }

    #[test]
    fn escapes_cell_text_and_sheet_names() {
        let html = render_sheets(&[sheet("<script>", &[&["<b>&</b>"]])], &Caps::default());
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("&lt;b&gt;&amp;&lt;/b&gt;"), "{html}");
    }

    #[test]
    fn truncates_at_the_row_cap_and_says_so() {
        let rows: Vec<&[&str]> = (0..10).map(|_| &["x"][..]).collect();
        let caps = Caps {
            max_rows_per_sheet: 4,
            ..Caps::default()
        };
        let html = render_sheets(&[sheet("S", &rows)], &caps);
        assert_eq!(html.matches("<tr>").count(), 4, "{html}");
        assert!(html.contains("sheet-truncated"), "{html}");
    }

    #[test]
    fn stops_at_the_total_cell_cap_across_sheets() {
        let caps = Caps {
            max_total_cells: 3,
            ..Caps::default()
        };
        let html = render_sheets(
            &[
                sheet("A", &[&["1", "2"], &["3", "4"]]),
                sheet("B", &[&["5", "6"]]),
            ],
            &caps,
        );
        assert!(html.contains("sheet-truncated"), "{html}");
        assert!(
            !html.contains(">5<"),
            "budget exhausted, B should not render: {html}"
        );
    }

    #[test]
    fn renders_an_empty_workbook_without_panicking() {
        let html = render_sheets(&[], &Caps::default());
        assert!(html.contains("sheet-empty"), "{html}");
    }

    #[test]
    fn pads_ragged_rows_to_the_widest_row() {
        let html = render_sheets(
            &[Sheet {
                name: "S".into(),
                rows: vec![
                    vec![Cell::Text("a".into())],
                    vec![Cell::Text("b".into()), Cell::Text("c".into())],
                ],
            }],
            &Caps::default(),
        );
        assert_eq!(html.matches("<th>").count(), 2, "header padded: {html}");
    }

    fn fixture() -> Vec<u8> {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let s1 = wb.add_worksheet();
        s1.set_name("Data").unwrap();
        s1.write_string(0, 0, "Item").unwrap();
        s1.write_string(0, 1, "Qty").unwrap();
        s1.write_string(1, 0, "Bolt").unwrap();
        s1.write_number(1, 1, 120.0).unwrap();
        let s2 = wb.add_worksheet();
        s2.set_name("Notes").unwrap();
        s2.write_string(0, 0, "<hello>").unwrap();
        wb.save_to_buffer().unwrap()
    }

    #[test]
    fn reads_every_sheet_of_a_real_workbook() {
        let html = render_workbook(&fixture(), &Caps::default()).unwrap();
        assert!(html.contains("<h2>Data</h2>"), "{html}");
        assert!(html.contains("<h2>Notes</h2>"), "{html}");
        assert!(html.contains("<th>Item</th>"), "{html}");
        assert!(html.contains("<td>Bolt</td>"), "{html}");
        assert!(html.contains("<td>120</td>"), "number formatting: {html}");
    }

    #[test]
    fn escapes_content_that_came_from_the_workbook() {
        let html = render_workbook(&fixture(), &Caps::default()).unwrap();
        assert!(!html.contains("<hello>"), "{html}");
        assert!(html.contains("&lt;hello&gt;"), "{html}");
    }

    #[test]
    fn rejects_a_workbook_over_the_byte_cap() {
        let caps = Caps {
            max_file_bytes: 16,
            ..Caps::default()
        };
        let err = render_workbook(&fixture(), &caps).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn reports_a_readable_error_for_a_corrupt_workbook() {
        let err = render_workbook(b"not a zip archive at all", &Caps::default()).unwrap_err();
        assert!(!err.is_empty());
        assert!(err.to_lowercase().contains("spreadsheet"), "{err}");
    }

    fn fixture_three_sheets() -> Vec<u8> {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        for name in ["A", "B", "C"] {
            let s = wb.add_worksheet();
            s.set_name(name).unwrap();
            s.write_string(0, 0, name).unwrap();
            s.write_string(0, 1, name).unwrap();
        }
        wb.save_to_buffer().unwrap()
    }

    #[test]
    fn stops_reading_sheets_once_the_cell_budget_is_exhausted() {
        let caps = Caps {
            max_total_cells: 2,
            ..Caps::default()
        };
        let html = render_workbook(&fixture_three_sheets(), &caps).unwrap();
        assert!(html.contains("<h2>A</h2>"), "{html}");
        assert!(
            !html.contains("<h2>B</h2>"),
            "B should never be read: {html}"
        );
        assert!(
            !html.contains("<h2>C</h2>"),
            "C should never be read: {html}"
        );
        assert!(html.contains("sheet-truncated"), "{html}");
    }

    fn fixture_long_cell(len: usize) -> Vec<u8> {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let s = wb.add_worksheet();
        s.write_string(0, 0, "<".repeat(len)).unwrap();
        wb.save_to_buffer().unwrap()
    }

    #[test]
    fn truncates_cell_text_to_the_configured_cap_and_still_escapes_it() {
        let caps = Caps {
            max_cell_chars: 10,
            ..Caps::default()
        };
        let html = render_workbook(&fixture_long_cell(50), &caps).unwrap();
        assert_eq!(html.matches("&lt;").count(), 10, "{html}");
    }
}
