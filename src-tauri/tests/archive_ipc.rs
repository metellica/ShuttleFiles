//! End-to-end checks of the archive IPC surface: browsing an archive
//! like a folder, opening a member, and the extract / compress jobs.
//!
//! The virtual `archive!\inner` paths only exist at the command layer —
//! `list_dir` has to recognise them, `resolve_path` has to turn an
//! archive file into one, and the job commands have to accept them where
//! a real folder would go. None of that is visible to the unit tests.

use std::path::{Path, PathBuf};
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
            shuttle_files_lib::commands::filesystem::list_dir,
            shuttle_files_lib::commands::filesystem::resolve_path,
            shuttle_files_lib::commands::filesystem::parent_path,
            shuttle_files_lib::commands::filesystem::breadcrumbs,
            shuttle_files_lib::commands::archive::archive_extensions,
            shuttle_files_lib::commands::archive::archive_open_member,
            shuttle_files_lib::commands::archive::archive_suggest_name,
            shuttle_files_lib::commands::operations::start_operation,
            shuttle_files_lib::commands::operations::list_operations,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");

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

fn wait_for_finish(webview: &MockWebview, id: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(30);
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

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("shuttle-archive-ipc-{}", uuid::Uuid::new_v4()));
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

/// `tree/a.txt` + `tree/sub/b.txt`, packed into `<dir>/pkg.<ext>` by a
/// compress job over IPC.
fn pack(webview: &MockWebview, dir: &Path, extension: &str) -> PathBuf {
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("sub")).unwrap();
    std::fs::write(tree.join("a.txt"), b"aaaa").unwrap();
    std::fs::write(tree.join("sub").join("b.txt"), b"bbbbbb").unwrap();

    let archive = dir.join(format!("pkg.{}", extension));
    let id = invoke(
        webview,
        "start_operation",
        json!({
            "kind": "compress",
            "sources": [tree.to_string_lossy()],
            "destDir": dir.to_string_lossy(),
            "options": { "archivePath": archive.to_string_lossy(), "level": 6 },
        }),
    );
    let id = id.as_str().expect("job id").to_string();
    let job = wait_for_finish(webview, &id);
    assert_eq!(job["status"], "completed", "compress job: {:?}", job);
    assert_eq!(job["totalFiles"], 2);
    assert!(archive.is_file(), "the archive was written");
    archive
}

#[test]
fn an_archive_browses_like_a_folder_over_ipc() {
    let tmp = TempDir::new();
    let (_app, webview) = build_app();
    let archive = pack(&webview, tmp.path(), "zip");

    // Pointing the address bar at the file enters it.
    let root = invoke(
        &webview,
        "resolve_path",
        json!({ "input": archive.to_string_lossy() }),
    );
    let root = root.as_str().expect("archive root").to_string();
    assert_eq!(root, format!("{}!\\", archive.to_string_lossy()));

    let listing = invoke(&webview, "list_dir", json!({ "path": root }));
    assert_eq!(listing["displayName"], "pkg.zip");
    assert_eq!(listing["isVirtualRoot"], false);
    let entries = listing["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["name"], "tree");
    assert_eq!(entries[0]["isDir"], true);

    // One level down, with sizes and the virtual paths navigation uses.
    let inner = entries[0]["path"].as_str().unwrap().to_string();
    let level = invoke(&webview, "list_dir", json!({ "path": inner.clone() }));
    let rows = level["entries"].as_array().expect("entries");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "a.txt");
    assert_eq!(rows[0]["size"], 4);
    assert_eq!(rows[0]["ext"], "txt");
    assert_eq!(rows[1]["name"], "sub");
    assert_eq!(rows[1]["isDir"], true);

    // Leaving the archive lands back on disk.
    let parent = invoke(&webview, "parent_path", json!({ "path": inner }));
    assert_eq!(parent.as_str().unwrap(), root);
    let out = invoke(&webview, "parent_path", json!({ "path": root }));
    assert_eq!(out.as_str().unwrap(), tmp.path().to_string_lossy());
}

#[test]
fn opening_a_member_extracts_a_readable_copy() {
    let tmp = TempDir::new();
    let (_app, webview) = build_app();
    let archive = pack(&webview, tmp.path(), "7z");

    let member = format!("{}!\\tree\\sub\\b.txt", archive.to_string_lossy());
    let extracted = invoke(&webview, "archive_open_member", json!({ "path": member }));
    let extracted = extracted.as_str().expect("a path on disk");
    assert_eq!(std::fs::read(extracted).unwrap(), b"bbbbbb");
    let _ = std::fs::remove_file(extracted);
}

#[test]
fn extracting_a_selection_runs_as_a_job() {
    let tmp = TempDir::new();
    let (_app, webview) = build_app();
    let archive = pack(&webview, tmp.path(), "tar.gz");
    let dest = tmp.path().join("out");

    let id = invoke(
        &webview,
        "start_operation",
        json!({
            "kind": "extract",
            "sources": [format!("{}!\\tree\\sub", archive.to_string_lossy())],
            "destDir": dest.to_string_lossy(),
        }),
    );
    let id = id.as_str().expect("job id").to_string();
    let job = wait_for_finish(&webview, &id);

    assert_eq!(job["status"], "completed", "extract job: {:?}", job);
    assert_eq!(job["totalFiles"], 1);
    assert_eq!(job["totalBytes"], 6);
    assert_eq!(std::fs::read(dest.join("sub").join("b.txt")).unwrap(), b"bbbbbb");
    assert!(!dest.join("tree").exists(), "only the selection came out");
}

#[test]
fn the_frontend_gets_the_same_extension_list_the_backend_dispatches_on() {
    let (_app, webview) = build_app();
    let extensions = invoke(&webview, "archive_extensions", json!({}));
    let extensions: Vec<String> = extensions
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    for expected in ["zip", "7z", "tar.gz", "tar.zst"] {
        assert!(extensions.contains(&expected.to_string()), "missing {}", expected);
    }
}

#[test]
fn a_suggested_name_follows_the_selection() {
    let tmp = TempDir::new();
    let (_app, webview) = build_app();
    let file = tmp.path().join("report.txt");
    std::fs::write(&file, b"x").unwrap();

    let single = invoke(
        &webview,
        "archive_suggest_name",
        json!({
            "dir": tmp.path().to_string_lossy(),
            "sources": [file.to_string_lossy()],
            "extension": "tar.gz",
        }),
    );
    assert_eq!(single, "report.tar.gz");

    let many = invoke(
        &webview,
        "archive_suggest_name",
        json!({
            "dir": tmp.path().to_string_lossy(),
            "sources": [file.to_string_lossy(), tmp.path().to_string_lossy()],
            "extension": "zip",
        }),
    );
    let folder = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    assert_eq!(many, format!("{}.zip", folder));
}
