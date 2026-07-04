mod ai;
mod commands;
mod i18n;
mod md_template;
mod menu;
mod pasteboard;
mod protocol;
mod state;
mod thumbs;
mod trash_util;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["thumb-worker"])
                .build(),
        )
        .manage(state::AppState::default())
        .register_uri_scheme_protocol("harbly-asset", protocol::asset_protocol)
        .register_uri_scheme_protocol("harbly-thumb", protocol::thumb_protocol)
        .invoke_handler(tauri::generate_handler![
            commands::library_status,
            commands::default_library_path,
            commands::pick_folder,
            commands::library_init,
            commands::scan_library,
            commands::rescan,
            commands::dir_tree,
            commands::list_assets,
            commands::asset_get,
            commands::inbox_count,
            commands::asset_read_text,
            commands::asset_write,
            commands::asset_checkpoint,
            commands::asset_new_markdown,
            commands::import_paths,
            commands::pick_and_import,
            commands::search_assets,
            commands::asset_rename,
            commands::assets_move,
            commands::assets_trash,
            commands::reveal_asset,
            commands::open_in_browser,
            commands::reveal_folder,
            commands::create_folder,
            commands::folder_rename,
            commands::folder_delete,
            commands::folder_has_content,
            commands::folder_duplicate,
            commands::asset_duplicate,
            commands::list_versions,
            commands::restore_version,
            commands::request_thumbs,
            commands::set_tags,
            commands::all_tags,
            commands::assets_by_tag,
            commands::asset_allow_once,
            commands::export_asset,
            commands::export_folder,
            commands::thumbs_rebuild,
            commands::undo_op,
            commands::redo_op,
            commands::pasteboard_copy,
            commands::pasteboard_paste,
            commands::forward_edit_action,
            commands::set_language,
            commands::get_language,
            ai::ai_detect_agents,
            ai::ai_key_status,
            ai::ai_set_key,
            ai::ai_get_config,
            ai::ai_set_config,
            ai::ai_runs_list,
            ai::ai_sessions_list,
            ai::ai_session_create,
            ai::ai_session_delete,
            ai::ai_session_set_prefs,
            ai::ai_session_messages,
            ai::ai_send,
            ai::ai_cancel,
        ])
        .setup(|app| {
            use tauri::Manager;
            let lang = commands::saved_lang(app.handle());
            *app.state::<state::AppState>().lang.lock().unwrap() = lang.clone();
            menu::setup(app.handle(), &lang)?;
            commands::try_autoload(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Harbly failed to start");
}
