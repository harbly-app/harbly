mod ai;
mod commands;
mod hdoc_template;
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
            commands::asset_snapshot_text,
            commands::asset_new_markdown,
            commands::asset_new_hdoc,
            commands::export_hdoc_html,
            commands::import_paths,
            commands::pick_and_import,
            commands::search_assets,
            commands::asset_rename,
            commands::assets_move,
            commands::assets_trash,
            commands::reveal_asset,
            commands::open_in_browser,
            commands::preview_hdoc,
            commands::open_url,
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
            commands::set_favorite,
            commands::favorite_assets,
            commands::favorite_count,
            commands::asset_allow_once,
            commands::export_asset,
            commands::export_folder,
            commands::thumbs_rebuild,
            commands::undo_op,
            commands::redo_op,
            commands::pasteboard_copy,
            commands::pasteboard_paste,
            commands::forward_edit_action,
            commands::read_clipboard_image,
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
            ai::ai_session_restore,
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
            menu::attach_event_bridge(app.handle());
            commands::try_autoload(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Harbly failed to start")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                // macOS convention: closing the last window keeps the app in
                // the Dock. Note ⌘Q (native terminate:) never reaches
                // ExitRequested at all — it goes straight to RunEvent::Exit;
                // Some(code) here only comes from AppHandle::exit()/restart().
                // code == None marks exactly the "last window closed" path,
                // which is the one we intercept. macOS-only so a future
                // Windows/Linux port doesn't gain a windowless zombie process.
                #[cfg(target_os = "macos")]
                if code.is_none() {
                    api.prevent_exit();
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (code, api);
            }
            tauri::RunEvent::Reopen { .. } => {
                // Dock icon click. The hidden thumb-worker window can keep the
                // process alive after ⌘W destroys "main", so the main window
                // must be re-shown or rebuilt from its config here.
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                } else if let Some(cfg) =
                    app.config().app.windows.iter().find(|w| w.label == "main")
                {
                    if let Ok(builder) = tauri::WebviewWindowBuilder::from_config(app, cfg) {
                        let _ = builder.build();
                    }
                }
            }
            tauri::RunEvent::Exit => {
                // ⌘Q teardown. Tokio tasks are never dropped on process exit, so
                // the per-run kill paths (kill_on_drop, cancel checks) cannot
                // fire — without this hook a mid-turn claude/codex CLI (its own
                // process group, unsignalled on parent death) survives as an
                // orphan, keeps writing assets through MCP with no timeout, and
                // keeps burning the user's API quota.
                harbly_ai::kill_all_agent_groups();
            }
            _ => {}
        });
}
