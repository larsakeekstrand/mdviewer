#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use mdviewer_lib::launch::LaunchTarget;

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
        Some(raw) => match mdviewer_lib::launch::resolve_launch_path(&raw, &cwd)? {
            LaunchTarget::Folder(root) => Ok(mdviewer_lib::Startup {
                tree_root: Some(root),
                initial_file: None,
            }),
            LaunchTarget::File(file) => {
                let parent = file
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cwd.clone());
                Ok(mdviewer_lib::Startup {
                    tree_root: Some(parent),
                    initial_file: Some(file),
                })
            }
        },
    }
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--claude-hook") {
        mdviewer_lib::run_claude_hook();
        return ExitCode::SUCCESS;
    }
    if std::env::args().nth(1).as_deref() == Some("--mcp") {
        mdviewer_lib::run_mcp_proxy();
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
