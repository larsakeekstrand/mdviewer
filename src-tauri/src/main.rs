#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() {
    let prog = env::args().next().unwrap_or_else(|| "mdviewer".to_string());
    let prog = std::path::Path::new(&prog)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("mdviewer");
    eprintln!("usage: {prog} [file-or-directory]");
}

fn resolve_args() -> Result<mdviewer_lib::Startup, String> {
    let arg = env::args().nth(1);
    let cwd = env::current_dir().map_err(|e| format!("cannot read current directory: {e}"))?;

    match arg {
        None => Ok(mdviewer_lib::Startup {
            tree_root: None,
            initial_file: None,
        }),
        Some(raw) => {
            let path = PathBuf::from(&raw);
            let absolute = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            let canonical = absolute
                .canonicalize()
                .map_err(|e| format!("cannot open '{raw}': {e}"))?;

            let meta =
                std::fs::metadata(&canonical).map_err(|e| format!("cannot stat '{raw}': {e}"))?;

            if meta.is_dir() {
                Ok(mdviewer_lib::Startup {
                    tree_root: Some(canonical),
                    initial_file: None,
                })
            } else {
                let parent = canonical
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cwd.clone());
                Ok(mdviewer_lib::Startup {
                    tree_root: Some(parent),
                    initial_file: Some(canonical),
                })
            }
        }
    }
}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--claude-hook") {
        mdviewer_lib::run_claude_hook();
        return ExitCode::SUCCESS;
    }
    let startup = match resolve_args() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mdviewer: {e}");
            usage();
            return ExitCode::from(1);
        }
    };
    mdviewer_lib::run(startup);
    ExitCode::SUCCESS
}
