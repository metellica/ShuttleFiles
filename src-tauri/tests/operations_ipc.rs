//! End-to-end checks of the file-operation IPC surface, driven through
//! Tauri's mock runtime so no window is needed.
//!
//! Regression guard: `start_operation` is a *synchronous* command, so it
//! runs on a thread with no Tokio reactor installed. An earlier version
//! used `tokio::task::spawn_blocking` there and panicked — and because
//! the panic unwound through a WebView2 `extern "system"` callback, it
//! aborted the whole process instead of returning an error.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::ipc::CallbackFn;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindowBuilder};

use shuttle_files_lib::ops::OpsRegistry;

type MockApp = tauri::App<tauri::test::MockRuntime>;
type MockWebview = tauri::WebviewWindow<tauri::test::MockRuntime>;

fn build_app() -> (MockApp, MockWebview) {
    let app = mock_builder()
        .manage(OpsRegistry::default())
        .invoke_handler(tauri::generate_handler![
            shuttle_files_lib::commands::operations::start_operation,
            shuttle_files_lib::commands::operations::cancel_operation,
            shuttle_files_lib::commands::operations::list_operations,
            shuttle_files_lib::commands::operations::clear_finished_operations,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");

    // One webview per app: labels have to be unique.
    let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .expect("build webview");
    (app, webview)
}

fn request(cmd: &str, body: serde_json::Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: body.into(),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

fn invoke(webview: &MockWebview, cmd: &str, body: serde_json::Value) -> serde_json::Value {
    get_ipc_response(webview, request(cmd, body))
        .map(|b| b.deserialize::<serde_json::Value>().unwrap())
        .unwrap_or_else(|e| panic!("{} failed: {:?}", cmd, e))
}

/// Poll `list_operations` until the job leaves the running states.
fn wait_for_finish(webview: &MockWebview, id: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let jobs = invoke(webview, "list_operations", json!({}));
        let job = jobs
            .as_array()
            .expect("array of jobs")
            .iter()
            .find(|j| j["id"] == id)
            .cloned()
            .expect("job present in the registry");

        let status = job["status"].as_str().unwrap_or_default().to_string();
        if !matches!(status.as_str(), "scanning" | "running") {
            return job;
        }
        assert!(Instant::now() < deadline, "job never finished: {:?}", job);
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("shuttle-files-ipc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn copy_job_runs_to_completion_over_ipc() {
    let tmp = TempDir::new();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("one.txt"), b"hello").unwrap();
    std::fs::write(src.join("nested").join("two.txt"), b"world!").unwrap();

    let (_app, webview) = build_app();
    let id = invoke(
        &webview,
        "start_operation",
        json!({
            "kind": "copy",
            "sources": [src.to_string_lossy()],
            "destDir": dst.to_string_lossy(),
        }),
    );
    let id = id.as_str().expect("job id").to_string();

    let job = wait_for_finish(&webview, &id);
    assert_eq!(job["status"], "completed", "job: {:?}", job);
    assert_eq!(job["totalFiles"], 2);
    assert_eq!(job["doneFiles"], 2);
    assert_eq!(job["totalBytes"], 11);

    assert_eq!(
        std::fs::read(dst.join("src").join("one.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        std::fs::read(dst.join("src").join("nested").join("two.txt")).unwrap(),
        b"world!"
    );
    // A copy must leave the source alone.
    assert!(src.join("one.txt").exists());
}

#[test]
fn delete_job_runs_to_completion_over_ipc() {
    let tmp = TempDir::new();
    let victim = tmp.path().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("a.bin"), b"12345").unwrap();

    let (_app, webview) = build_app();
    let id = invoke(
        &webview,
        "start_operation",
        json!({ "kind": "delete", "sources": [victim.to_string_lossy()] }),
    );
    let id = id.as_str().expect("job id").to_string();

    let job = wait_for_finish(&webview, &id);
    assert_eq!(job["status"], "completed", "job: {:?}", job);
    assert!(!victim.exists());
}

#[test]
fn a_copy_without_a_destination_is_rejected() {
    let tmp = TempDir::new();
    let src = tmp.path().join("a.txt");
    std::fs::write(&src, b"x").unwrap();

    let (_app, webview) = build_app();

    let response = get_ipc_response(
        &webview,
        request(
            "start_operation",
            json!({ "kind": "copy", "sources": [src.to_string_lossy()] }),
        ),
    );
    assert!(response.is_err(), "expected a rejection, got {:?}", response);
}

#[test]
fn finished_jobs_are_cleared_on_request() {
    let tmp = TempDir::new();
    let victim = tmp.path().join("gone.txt");
    std::fs::write(&victim, b"bye").unwrap();

    let (_app, webview) = build_app();
    let id = invoke(
        &webview,
        "start_operation",
        json!({ "kind": "delete", "sources": [victim.to_string_lossy()] }),
    );
    let id = id.as_str().unwrap().to_string();
    wait_for_finish(&webview, &id);

    invoke(&webview, "clear_finished_operations", json!({}));
    let jobs = invoke(&webview, "list_operations", json!({}));
    assert!(
        jobs.as_array().unwrap().is_empty(),
        "finished jobs should be gone: {:?}",
        jobs
    );
}
