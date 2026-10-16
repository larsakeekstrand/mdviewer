use std::io::{Cursor, Read};
use std::path::Path;

// Bytes cross IPC as a JSON number array until the CSP gains `connect-src ipc:`, so keep this small.
pub const MAX_MODEL_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_3MF_DECLARED_BYTES: u64 = 512 * 1024 * 1024;

const BAMBU_REFUSAL: &str =
    "This looks like a Bambu Studio project file; only plain 3MF models are supported";

fn extension(p: &Path) -> Option<String> {
    p.extension().map(|e| e.to_string_lossy().to_lowercase())
}

pub fn is_model_path(p: &Path) -> bool {
    matches!(extension(p).as_deref(), Some("stl" | "3mf" | "obj"))
}

pub fn check_size(len: u64, max: u64) -> Result<(), String> {
    if len > max {
        return Err(format!(
            "model file is too large ({} MB; limit {} MB)",
            len / (1024 * 1024),
            max / (1024 * 1024)
        ));
    }
    Ok(())
}

pub fn is_bambu_project<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    names.into_iter().any(|n| {
        n.starts_with("3D/Objects/") || n.eq_ignore_ascii_case("Metadata/project_settings.config")
    })
}

pub fn check_3mf(bytes: &[u8], max_declared: u64) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("not a valid 3MF archive: {e}"))?;
    let mut names = Vec::with_capacity(zip.len());
    let mut declared: u64 = 0;
    for i in 0..zip.len() {
        let f = zip
            .by_index_raw(i)
            .map_err(|e| format!("not a valid 3MF archive: {e}"))?;
        declared = declared.saturating_add(f.size());
        names.push(f.name().to_string());
    }
    if is_bambu_project(names.iter().map(String::as_str)) {
        return Err(BAMBU_REFUSAL.to_string());
    }
    if declared > max_declared {
        let mb = 1024 * 1024;
        return Err(format!(
            "3MF declares too much uncompressed data ({} MB; limit {} MB)",
            declared / mb,
            max_declared / mb
        ));
    }
    Ok(())
}

fn read_capped(reader: impl Read, max: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    reader
        .take(max + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read model: {e}"))?;
    check_size(buf.len() as u64, max)?;
    Ok(buf)
}

pub fn read_model_bytes(p: &Path) -> Result<Vec<u8>, String> {
    if !is_model_path(p) {
        return Err(format!("not a 3D model file: {}", p.display()));
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    if !meta.is_file() {
        return Err(format!("not a regular file: {}", p.display()));
    }
    check_size(meta.len(), MAX_MODEL_BYTES)?;
    let file = std::fs::File::open(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    let bytes = read_capped(file, MAX_MODEL_BYTES)?;
    if extension(p).as_deref() == Some("3mf") {
        check_3mf(&bytes, MAX_3MF_DECLARED_BYTES)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    const BAMBU: &str =
        "This looks like a Bambu Studio project file; only plain 3MF models are supported";

    fn zip_of(names: &[&str]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for n in names {
            w.start_file(*n, opts).unwrap();
            w.write_all(b"<x/>").unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mdv-model-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    #[test]
    fn is_model_path_accepts_print_formats() {
        for ok in ["a.stl", "B.STL", "c.3mf", "d.3MF", "e.obj"] {
            assert!(is_model_path(Path::new(ok)), "{ok}");
        }
        for no in ["a.step", "a.scad", "a.stl.exe", "a", "a.stlx", "a.md"] {
            assert!(!is_model_path(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn check_size_rejects_above_cap() {
        assert!(check_size(10, 10).is_ok());
        assert!(check_size(11, 10).unwrap_err().contains("too large"));
    }

    #[test]
    fn is_bambu_project_detects_objects_dir_and_project_settings() {
        let plain = ["3D/3dmodel.model", "[Content_Types].xml"];
        assert!(!is_bambu_project(plain));
        assert!(is_bambu_project(
            plain.into_iter().chain(["3D/Objects/object_1.model"])
        ));
        assert!(is_bambu_project(["Metadata/project_settings.config"]));
    }

    #[test]
    fn check_3mf_accepts_plain_archive() {
        let z = zip_of(&["[Content_Types].xml", "3D/3dmodel.model"]);
        assert!(check_3mf(&z, MAX_3MF_DECLARED_BYTES).is_ok());
    }

    #[test]
    fn check_3mf_refuses_bambu_project() {
        let z = zip_of(&[
            "[Content_Types].xml",
            "3D/3dmodel.model",
            "3D/Objects/object_1.model",
        ]);
        assert_eq!(check_3mf(&z, MAX_3MF_DECLARED_BYTES).unwrap_err(), BAMBU);
    }

    #[test]
    fn check_3mf_refuses_declared_bytes_over_cap() {
        let z = zip_of(&["[Content_Types].xml", "3D/3dmodel.model"]);
        assert!(check_3mf(&z, 1)
            .unwrap_err()
            .contains("too much uncompressed data"));
    }

    #[test]
    fn check_3mf_refuses_non_zip() {
        assert!(check_3mf(b"solid x", MAX_3MF_DECLARED_BYTES)
            .unwrap_err()
            .starts_with("not a valid 3MF archive"));
    }

    #[test]
    fn read_model_bytes_refuses_non_model_extension() {
        let p = temp_file("x.md", b"# hi");
        assert!(read_model_bytes(&p)
            .unwrap_err()
            .starts_with("not a 3D model file"));
    }

    #[cfg(unix)]
    #[test]
    fn read_model_bytes_refuses_non_regular_file() {
        let d = std::env::temp_dir().join(format!("mdv-model-link-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let link = d.join("x.stl");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/dev/null", &link).unwrap();
        assert!(read_model_bytes(&link)
            .unwrap_err()
            .starts_with("not a regular file"));
    }

    #[test]
    fn read_capped_rejects_over_cap_content() {
        assert!(read_capped(Cursor::new(vec![0u8; 10]), 10).is_ok());
        assert!(read_capped(Cursor::new(vec![0u8; 11]), 10)
            .unwrap_err()
            .contains("too large"));
    }

    #[test]
    fn read_model_bytes_reads_stl() {
        let p = temp_file("x.stl", b"solid t\nendsolid t\n");
        assert_eq!(read_model_bytes(&p).unwrap(), b"solid t\nendsolid t\n");
    }
}
