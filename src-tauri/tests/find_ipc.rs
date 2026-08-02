//! End-to-end checks of the fuzzy-find and checksum IPC surface.
//!
//! These commands cross the serde boundary in ways the unit tests do not
//! exercise: `SearchHit` flattens a `FileEntry` into itself and renames
//! to camelCase, and `HashAlgo` has to deserialize from the plain strings
//! the frontend sends. A mistake in either is invisible to Rust callers
//! but breaks the UI, so it is checked from the outside here.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::ipc::CallbackFn;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Listener, WebviewUrl, WebviewWindowBuilder};

use shuttle_files_lib::cancel::{HashCancels, SearchCancels};

type MockApp = tauri::App<tauri::test::MockRuntime>;
type MockWebview = tauri::WebviewWindow<tauri::test::MockRuntime>;

fn build_app() -> (MockApp, MockWebview) {
    let app = mock_builder()
        .manage(SearchCancels::default())
        .manage(HashCancels::default())
        .invoke_handler(tauri::generate_handler![
            shuttle_files_lib::commands::find::fuzzy_find,
            shuttle_files_lib::commands::find::cancel_search,
            shuttle_files_lib::commands::find::start_hash,
            shuttle_files_lib::commands::find::cancel_hash,
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

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("shuttle-files-find-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn file(&self, rel: &str, body: &[u8]) -> String {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> TempDir {
    let tmp = TempDir::new();
    tmp.file("readme.md", b"x");
    tmp.file("cargo.toml", b"x");
    tmp.file("src/main.rs", b"x");
    tmp.file("src/deep/buried.rs", b"x");
    tmp
}

fn find(webview: &MockWebview, dir: &Path, query: &str, recursive: bool) -> serde_json::Value {
    invoke(
        webview,
        "fuzzy_find",
        json!({
            "id": "test-search",
            "dir": dir.to_string_lossy(),
            "query": query,
            "recursive": recursive,
        }),
    )
}

#[test]
fn a_recursive_find_returns_camel_cased_hits_over_ipc() {
    let tmp = fixture();
    let (_app, webview) = build_app();

    let result = find(&webview, tmp.path(), "buried", true);
    let hits = result["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1, "got {:?}", result);

    let hit = &hits[0];
    // The flattened FileEntry has to arrive on the hit itself, in the
    // camelCase shape the frontend's FileEntry type expects.
    assert_eq!(hit["name"], "buried.rs");
    assert_eq!(hit["ext"], "rs");
    assert_eq!(hit["isDir"], false);
    assert!(hit["isHidden"].is_boolean());
    assert!(hit["isSymlink"].is_boolean());
    assert!(hit["modified"].is_number());
    assert!(hit["path"].as_str().unwrap().ends_with("buried.rs"));
    // ...alongside the search-specific fields.
    assert!(hit["rel"].as_str().unwrap().ends_with("buried.rs"));
    assert!(hit["score"].is_number());
    assert!(hit["positions"].is_array());

    assert_eq!(result["total"], 1);
    assert_eq!(result["cancelled"], false);
    assert_eq!(result["truncated"], false);
}

#[test]
fn a_shallow_find_does_not_descend() {
    let tmp = fixture();
    let (_app, webview) = build_app();

    assert_eq!(find(&webview, tmp.path(), "buried", false)["total"], 0);
    assert_eq!(find(&webview, tmp.path(), "cargo", false)["total"], 1);
}

#[test]
fn the_same_query_ranks_identically_shallow_and_recursive() {
    let tmp = fixture();
    let (_app, webview) = build_app();

    let shallow = find(&webview, tmp.path(), "cargo", false);
    let deep = find(&webview, tmp.path(), "cargo", true);
    assert_eq!(shallow["hits"][0]["score"], deep["hits"][0]["score"]);
}

#[test]
fn an_empty_query_returns_no_hits_rather_than_the_whole_tree() {
    let tmp = fixture();
    let (_app, webview) = build_app();

    let result = find(&webview, tmp.path(), "   ", true);
    assert_eq!(result["hits"].as_array().unwrap().len(), 0);
    assert_eq!(result["total"], 0);
}

#[test]
fn a_search_in_a_missing_directory_reports_an_error() {
    let (_app, webview) = build_app();
    let response = get_ipc_response(
        &webview,
        request(
            "fuzzy_find",
            json!({
                "id": "test-search",
                "dir": "Z:\\definitely\\not\\here",
                "query": "x",
                "recursive": false,
            }),
        ),
    );
    assert!(response.is_err(), "a missing directory must not succeed");
}

#[test]
fn cancelling_an_idle_search_is_harmless() {
    let (_app, webview) = build_app();
    invoke(&webview, "cancel_search", json!({ "id": "never-started" }));
}

/// Collect hash events until `hash:finished` arrives.
fn run_hash(
    app: &MockApp,
    webview: &MockWebview,
    id: &str,
    paths: Vec<String>,
    algos: serde_json::Value,
) -> (Vec<serde_json::Value>, bool) {
    let (tx, rx) = mpsc::channel::<serde_json::Value>();
    let result_tx = tx.clone();
    app.listen("hash:result", move |event| {
        let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        let _ = result_tx.send(json!({ "kind": "result", "payload": payload }));
    });
    app.listen("hash:finished", move |event| {
        let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        let _ = tx.send(json!({ "kind": "finished", "payload": payload }));
    });

    invoke(
        webview,
        "start_hash",
        json!({ "id": id, "paths": paths, "algos": algos }),
    );

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut results = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "hash job never finished");
        match rx.recv_timeout(remaining) {
            Ok(event) if event["kind"] == "result" => {
                results.push(event["payload"]["result"].clone())
            }
            Ok(event) => {
                assert_eq!(event["payload"]["id"], id);
                return (results, event["payload"]["cancelled"].as_bool().unwrap());
            }
            Err(e) => panic!("hash job never finished: {:?}", e),
        }
    }
}

#[test]
fn hashing_streams_results_back_as_events() {
    let tmp = TempDir::new();
    let path = tmp.file("abc.txt", b"abc");
    let (app, webview) = build_app();

    let (results, cancelled) = run_hash(
        &app,
        &webview,
        "hash-1",
        vec![path.clone()],
        json!(["md5", "sha256"]),
    );

    assert!(!cancelled);
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert_eq!(r["name"], "abc.txt");
    assert_eq!(r["size"], 3);
    assert_eq!(r["error"], "");
    assert_eq!(r["md5"], "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        r["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn only_the_algorithms_the_frontend_asks_for_are_computed() {
    let tmp = TempDir::new();
    let path = tmp.file("abc.txt", b"abc");
    let (app, webview) = build_app();

    let (results, _) = run_hash(&app, &webview, "hash-2", vec![path], json!(["sha256"]));
    assert_eq!(results[0]["md5"], "");
    assert_eq!(
        results[0]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn every_file_in_a_batch_is_reported_even_when_one_is_unreadable() {
    let tmp = TempDir::new();
    let good = tmp.file("good.txt", b"abc");
    let missing = tmp.path().join("nope.txt").to_string_lossy().to_string();
    let (app, webview) = build_app();

    let (results, cancelled) = run_hash(
        &app,
        &webview,
        "hash-3",
        vec![missing, good],
        json!(["md5"]),
    );

    assert!(!cancelled);
    assert_eq!(results.len(), 2);
    assert_ne!(results[0]["error"], "");
    assert_eq!(results[1]["error"], "");
    assert_eq!(results[1]["md5"], "900150983cd24fb0d6963f7d28e17f72");
}

#[test]
fn an_unknown_algorithm_is_rejected_rather_than_silently_ignored() {
    let tmp = TempDir::new();
    let path = tmp.file("abc.txt", b"abc");
    let (_app, webview) = build_app();

    let response = get_ipc_response(
        &webview,
        request(
            "start_hash",
            json!({ "id": "hash-4", "paths": [path], "algos": ["sha3"] }),
        ),
    );
    assert!(response.is_err(), "an unknown digest must not be accepted");
}

#[test]
fn cancelling_an_idle_hash_is_harmless() {
    let (_app, webview) = build_app();
    invoke(&webview, "cancel_hash", json!({ "id": "never-started" }));
}
