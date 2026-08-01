//! Integration tests for the out-of-process shell context menu helper.
//!
//! These exercise the real COM path — `SHParseDisplayName` →
//! `SHBindToParent` → `GetUIObjectOf` → `QueryContextMenu` — against
//! whatever extensions are installed on the machine, without ever
//! displaying a menu. That is the whole reason the helper has a `list`
//! mode: the interesting part is testable headlessly.
//!
//! Nothing here asserts that a *specific* extension is present, since
//! that varies per machine. What is asserted is the contract: the shell
//! menu resolves, contains the built-in verbs, nests submenus, and
//! reports errors instead of hanging or panicking.

#![cfg(windows)]

use std::path::{Path, PathBuf};

use shuttle_files_lib::shell::menu::{self, ShellMenuItem};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("shuttle-files-menu-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn file(&self, name: &str, body: &[u8]) -> String {
        let p = self.0.join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The helper lives next to the test binary (`target/<profile>/`).
fn helper_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()? // .../deps
        .parent()
        .map(Path::to_path_buf)
}

/// Skip rather than fail when the helper has not been built: running
/// `cargo test --test shellmenu_ipc` alone does not build other bins.
fn helper_available() -> bool {
    helper_dir().is_some_and(|d| d.join("shellmenu.exe").is_file())
}

fn list(paths: &[String]) -> Result<Vec<ShellMenuItem>, String> {
    // Point the lookup at the build directory; `current_exe` inside the
    // test process is the test binary in `deps/`.
    let dir = helper_dir();
    menu::list(dir, paths)
        .map(|r| r.items)
        .map_err(|e| e.to_string())
}

fn flatten(items: &[ShellMenuItem], out: &mut Vec<ShellMenuItem>) {
    for item in items {
        out.push(item.clone());
        flatten(&item.children, out);
    }
}

fn all_items(items: &[ShellMenuItem]) -> Vec<ShellMenuItem> {
    let mut out = Vec::new();
    flatten(items, &mut out);
    out
}

fn has_verb(items: &[ShellMenuItem], verb: &str) -> bool {
    all_items(items).iter().any(|i| i.verb == verb)
}

#[test]
fn resolves_the_shell_menu_for_a_file() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let file = tmp.file("sample.txt", b"hello");

    let items = list(&[file]).expect("shell menu");
    assert!(!items.is_empty(), "the shell menu should never be empty");

    // Verbs every Windows install provides for a plain file.
    for verb in ["open", "copy", "cut", "delete", "properties"] {
        assert!(has_verb(&items, verb), "missing built-in verb '{}'", verb);
    }
}

#[test]
fn resolves_the_shell_menu_for_a_folder() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let sub = tmp.path().join("child");
    std::fs::create_dir_all(&sub).unwrap();

    let items = list(&[sub.to_string_lossy().to_string()]).expect("shell menu");
    assert!(has_verb(&items, "properties"), "folder menu looks wrong");
}

#[test]
fn multi_selection_produces_a_menu() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let a = tmp.file("a.txt", b"a");
    let b = tmp.file("b.txt", b"b");

    let items = list(&[a, b]).expect("shell menu");
    assert!(has_verb(&items, "copy"), "multi-selection menu looks wrong");
}

#[test]
fn submenus_are_walked() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let file = tmp.file("nested.txt", b"x");

    let items = list(&[file]).expect("shell menu");
    // "Send to" is present on every Windows install and always has
    // children, so it proves recursion works without depending on any
    // particular third-party extension.
    let parent = items
        .iter()
        .chain(all_items(&items).iter())
        .find(|i| !i.children.is_empty())
        .cloned();
    assert!(parent.is_some(), "no submenu found; recursion likely broken");
}

#[test]
fn separators_and_ids_are_well_formed() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let file = tmp.file("ids.txt", b"x");
    let items = all_items(&list(&[file]).expect("shell menu"));

    for item in &items {
        if item.separator {
            assert!(item.id.is_none(), "a separator must not carry a command id");
            assert!(item.label.is_empty(), "a separator must not carry a label");
        } else {
            assert!(!item.label.is_empty(), "every command needs a label");
            // Either it runs a command, or it opens a submenu. The
            // submenu may still be empty here: extensions like KDiff3
            // only build theirs when the menu is actually displayed.
            assert!(
                item.id.is_some() || item.has_submenu,
                "item '{}' can neither be invoked nor expanded",
                item.label
            );
        }
    }
}

#[test]
fn items_from_different_folders_are_rejected() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let a = tmp.file("a.txt", b"a");

    let err = list(&[a, "C:\\Windows\\win.ini".to_string()]).unwrap_err();
    assert!(err.contains("same folder"), "unexpected error: {}", err);
}

#[test]
fn a_missing_path_reports_an_error() {
    if !helper_available() {
        eprintln!("skipping: shellmenu.exe not built");
        return;
    }
    let tmp = TempDir::new();
    let ghost = tmp.path().join("does-not-exist.txt");

    let err = list(&[ghost.to_string_lossy().to_string()]).unwrap_err();
    assert!(!err.is_empty(), "a missing file must produce an error");
}

#[test]
fn an_empty_selection_is_rejected_without_spawning() {
    let err = menu::list(helper_dir(), &[]).unwrap_err().to_string();
    assert!(err.contains("Nothing selected"), "unexpected error: {}", err);
}

#[test]
fn a_missing_helper_is_reported_not_panicked() {
    // Points the lookup at a directory with no helper in it, which is
    // what a broken install would look like.
    let empty = std::env::temp_dir().join(format!("shuttle-files-nohelper-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&empty).unwrap();

    // `current_exe`'s directory is still searched first, so this only
    // proves the error path when the helper really is absent there.
    let result = menu::list(Some(empty.clone()), &["C:\\Windows\\win.ini".to_string()]);
    let _ = std::fs::remove_dir_all(&empty);

    match result {
        // Helper present next to the test binary: the call succeeds.
        Ok(r) => assert!(!r.items.is_empty()),
        // Helper absent: a readable error, never a panic.
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("shellmenu") || msg.contains("helper"),
                "unhelpful error: {}",
                msg
            );
        }
    }
}
