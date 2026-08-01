pub mod commands;
pub mod config;
pub mod error;
pub mod fs;
pub mod ops;
pub mod shell;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(ops::OpsRegistry::default())
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
            commands::clipboard::clipboard_write_files,
            commands::clipboard::clipboard_read_files,
            commands::clipboard::clipboard_has_files,
            commands::shellmenu::shell_menu_show,
            commands::shellmenu::shell_menu_list,
            commands::operations::start_operation,
            commands::operations::cancel_operation,
            commands::operations::list_operations,
            commands::operations::clear_finished_operations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShuttleFiles");
}
