pub mod cancel;
pub mod archive;
pub mod commands;
pub mod config;
pub mod error;
pub mod fs;
pub mod ops;
pub mod shell;
pub mod terminal;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    // Write panics to a file so we can diagnose crashes in release mode.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{}\n{:?}", info, std::backtrace::Backtrace::force_capture());
        let path = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("shuttle-files-panic.log");
        let _ = std::fs::write(&path, &msg);
        eprintln!("PANIC: {}", msg);
    }));
    // Before any thread exists, and before anything can be launched: a
    // ShuttleFiles started from a VS Code terminal would otherwise pass
    // that terminal's variables on to every program it opens, and an
    // editor started with them never shows a window.
    shell::vscode::purge_inherited_vars();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|_app| {
            // Members opened for viewing are extracted to a scratch
            // folder nothing can delete while the viewer may still hold
            // it. Sweeping earlier runs' leftovers at start keeps the
            // temp directory from growing without bound.
            std::thread::spawn(archive::clean_scratch);
            Ok(())
        })
        .manage(ops::OpsRegistry::default())
        .manage(cancel::SearchCancels::default())
        .manage(cancel::HashCancels::default())
        .manage(terminal::TerminalManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::filesystem::list_dir,
            commands::filesystem::resolve_path,
            commands::filesystem::parent_path,
            commands::filesystem::breadcrumbs,
            commands::filesystem::home_dir,
            commands::filesystem::create_dir,
            commands::filesystem::rename_entry,
            commands::places::list_drives,
            commands::places::quick_access,
            commands::places::list_favorites,
            commands::places::add_favorite,
            commands::places::remove_favorite,
            commands::places::reorder_favorites,
            commands::places::list_recent,
            commands::places::record_visit,
            commands::places::clear_recent,
            commands::session::load_tabs,
            commands::session::save_tabs,
            commands::session::load_view_settings,
            commands::session::save_view_settings,
            commands::openwith::load_open_with,
            commands::openwith::save_open_with,
            commands::openwith::default_open_with,
            commands::openwith::open_entry,
            commands::openwith::vscode_available,
            commands::openwith::open_in_vscode,
            commands::find::fuzzy_find,
            commands::find::cancel_search,
            commands::find::start_hash,
            commands::find::cancel_hash,
            commands::clipboard::clipboard_write_files,
            commands::clipboard::clipboard_read_files,
            commands::clipboard::clipboard_has_files,
            commands::clipboard::clipboard_write_text,
            commands::clipboard::clipboard_read_text,
            commands::shellmenu::shell_menu_show,
            commands::shellmenu::shell_menu_list,
            commands::archive::archive_extensions,
            commands::archive::archive_open_member,
            commands::archive::archive_suggest_name,
            commands::operations::start_operation,
            commands::operations::cancel_operation,
            commands::operations::list_operations,
            commands::operations::clear_finished_operations,
            commands::terminal::list_terminals,
            commands::terminal::open_terminal,
            commands::terminal::terminal_reserve,
            commands::terminal::terminal_open,
            commands::terminal::terminal_input,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShuttleFiles");
}
