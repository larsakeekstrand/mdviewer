//! PDF export. On macOS, print the main webview to a file through the native
//! print pipeline (paginated) with the print/progress panels suppressed.

/// Print the calling window's webview to `path` as a PDF.
///
/// Async on purpose: Tauri runs sync commands on the main thread, but this
/// command must run off the main thread (it waits for the print, which runs ON
/// the main thread) — a sync command would deadlock.
#[tauri::command]
pub async fn export_pdf(window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::export(window, path).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, path);
        Err("PDF export is not yet supported on Windows".to_string())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPrintInfo, NSPrintJobSavingURL, NSPrintSaveJob, NSWindow};
    use objc2_foundation::{NSCopying, NSPoint, NSRect, NSString, NSURL};
    use objc2_web_kit::WKWebView;

    /// Poll interval / overall timeout for the asynchronous print to land a
    /// complete PDF on disk.
    const POLL_INTERVAL: Duration = Duration::from_millis(100);
    const PRINT_TIMEOUT: Duration = Duration::from_secs(30);

    pub async fn export(window: tauri::WebviewWindow, path: String) -> Result<(), String> {
        // Remove any stale file so the completion poll below can't mistake a
        // previous run's PDF for this one.
        let _ = std::fs::remove_file(&path);

        let (started_tx, started_rx) = mpsc::channel::<Result<(), String>>();
        let p = path.clone();
        // with_webview runs the closure on the main thread, which the print
        // machinery requires. The closure only *starts* the (asynchronous)
        // print and returns — it must not block the main thread, because the
        // print itself completes on the main runloop.
        window
            .with_webview(move |pw| {
                let r = unsafe { start_print(pw.inner(), pw.ns_window(), &p) };
                let _ = started_tx.send(r);
            })
            .map_err(|e| format!("with_webview failed: {e}"))?;

        // Propagate any error from kicking off the print.
        started_rx
            .recv()
            .map_err(|e| format!("print task dropped: {e}"))??;

        // Wait (off the main thread) until the print has written a complete PDF.
        // This also gates the frontend's view-restore until capture is done.
        wait_for_complete_pdf(Path::new(&path), PRINT_TIMEOUT)
    }

    /// Build the save-to-file print operation and start it. Returns once the
    /// operation has been kicked off (it then runs asynchronously on the main
    /// runloop); it does NOT wait for completion.
    ///
    /// # Safety
    /// `webview_ptr` / `ns_window_ptr` must be the `WKWebView` / `NSWindow`
    /// pointers from `PlatformWebview`, called on the main thread.
    unsafe fn start_print(
        webview_ptr: *mut std::ffi::c_void,
        ns_window_ptr: *mut std::ffi::c_void,
        path: &str,
    ) -> Result<(), String> {
        if webview_ptr.is_null() {
            return Err("null webview pointer".to_string());
        }
        if ns_window_ptr.is_null() {
            return Err("null window pointer".to_string());
        }
        let webview: &WKWebView = &*(webview_ptr as *mut WKWebView);
        let window: &NSWindow = &*(ns_window_ptr as *mut NSWindow);

        // Save-to-file print info: disposition = save, destination URL in the
        // settings dictionary.
        let info = NSPrintInfo::new();
        info.setJobDisposition(NSPrintSaveJob);
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let key = ProtocolObject::<dyn NSCopying>::from_ref(NSPrintJobSavingURL);
        info.dictionary().setObject_forKey(url.as_ref(), key);

        let op = webview.printOperationWithPrintInfo(&info);
        op.setShowsPrintPanel(false);
        op.setShowsProgressPanel(false);

        // Required: give the print operation's view a real frame. Without it,
        // WKWebView printing crashes or emits blank pages.
        if let Some(view) = op.view() {
            view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), info.paperSize()));
        }

        // runOperation() does NOT work for WKWebView — it captures before the
        // asynchronous print rendering completes, producing endless blank pages.
        // runOperationModalForWindow runs the print on the main runloop and lets
        // that rendering finish. With panels off, no UI is shown; a nil delegate
        // is fine because we detect completion by polling the output file.
        op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
            window,
            None,
            None,
            std::ptr::null_mut(),
        );
        Ok(())
    }

    /// Block until `path` is a complete PDF (its tail contains `%%EOF`) or the
    /// timeout elapses. Reads only the file tail so a transiently large/partial
    /// file isn't slurped whole.
    fn wait_for_complete_pdf(path: &Path, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            if pdf_is_complete(path) {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err("PDF export timed out (no complete file written)".to_string());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// True if the file exists and its last bytes contain the PDF end marker.
    fn pdf_is_complete(path: &Path) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = std::fs::File::open(path) else {
            return false;
        };
        let Ok(len) = f.seek(SeekFrom::End(0)) else {
            return false;
        };
        if len < 6 {
            return false;
        }
        let tail = len.min(2048);
        if f.seek(SeekFrom::End(-(tail as i64))).is_err() {
            return false;
        }
        let mut buf = vec![0u8; tail as usize];
        if f.read_exact(&mut buf).is_err() {
            return false;
        }
        buf.windows(5).any(|w| w == b"%%EOF")
    }
}
