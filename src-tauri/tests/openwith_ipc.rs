//! End-to-end checks of the open/launch IPC surface.
//!
//! These commands are the ones a mistyped argument name would break
//! invisibly: the Rust side compiles, the UI calls it, and the only
//! symptom is that nothing opens. Invoking them through the real IPC
//! layer, with the payload the frontend sends, is what catches that.
//!
//! Nothing here starts a program. Each command is given input it must
//! reject, and the test asserts the rejection is the command's own —
//! proof the arguments arrived — rather than Tauri's "missing key".

use serde_json::json;
use tauri::ipc::CallbackFn;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindowBuilder};

type MockApp = tauri::App<tauri::test::MockRuntime>;
type MockWebview = tauri::WebviewWindow<tauri::test::MockRuntime>;

fn build_app() -> (MockApp, MockWebview) {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            shuttle_files_lib::commands::openwith::open_entry,
            shuttle_files_lib::commands::openwith::open_in_vscode,
            shuttle_files_lib::commands::openwith::vscode_available,
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

fn invoke(webview: &MockWebview, cmd: &str, body: serde_json::Value) -> Result<String, String> {
    match get_ipc_response(webview, request(cmd, body)) {
        Ok(b) => Ok(b.deserialize::<serde_json::Value>().unwrap().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// A rejection Tauri produced before the command ran means the payload
/// never bound — the failure this whole file exists to catch.
fn assert_reached_the_command(err: &str, cmd: &str) {
    let lower = err.to_lowercase();
    assert!(
        !lower.contains("missing required key") && !lower.contains("invalid args"),
        "{} never saw its arguments: {}",
        cmd,
        err
    );
}

#[test]
fn open_entry_receives_path_and_program() {
    let (_app, webview) = build_app();
    let ghost = std::env::temp_dir().join("shuttle-files-no-such-thing.txt");

    let err = invoke(
        &webview,
        "open_entry",
        json!({ "path": ghost.to_string_lossy(), "program": null }),
    )
    .expect_err("a missing file must be rejected");

    assert_reached_the_command(&err, "open_entry");
    assert!(err.contains("No such file"), "unexpected error: {}", err);
}

/// The UI sends `program` as an explicit `null` for the system default;
/// omitting it entirely has to work too, since `Option` is optional.
#[test]
fn open_entry_tolerates_an_absent_program() {
    let (_app, webview) = build_app();
    let ghost = std::env::temp_dir().join("shuttle-files-no-such-thing.txt");

    let err = invoke(&webview, "open_entry", json!({ "path": ghost.to_string_lossy() }))
        .expect_err("a missing file must be rejected");

    assert_reached_the_command(&err, "open_entry");
}

#[test]
fn open_in_vscode_receives_the_path_list() {
    let (_app, webview) = build_app();

    // An empty list is the one input that cannot start anything, so the
    // binding can be proven without opening an editor.
    let err = invoke(&webview, "open_in_vscode", json!({ "paths": [] }))
        .expect_err("an empty selection must be rejected");

    assert_reached_the_command(&err, "open_in_vscode");
    assert!(
        err.contains("Nothing selected"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn vscode_available_answers_without_arguments() {
    let (_app, webview) = build_app();
    let answer = invoke(&webview, "vscode_available", json!({})).expect("query");
    assert!(answer == "true" || answer == "false", "unexpected: {}", answer);
}
