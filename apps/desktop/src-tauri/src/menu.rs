use crate::i18n;
use tauri::menu::{
    AboutMetadataBuilder, Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter};

/// Native menu bar: all actions are bridged to the frontend via the "menu-action" event.
/// Undo/Redo/Copy/Paste/Select All/Move to Trash are all custom items — the frontend
/// routes by focus: inside an input field the action is forwarded back to the system
/// responder chain (text editing works as usual), otherwise it acts on files (Finder
/// semantics). On language switch, the menu is rebuilt wholesale with the matching strings.
///
/// Rebuilding must NOT re-attach the event bridge: `on_menu_event` stacks
/// handlers instead of replacing, so every set_language (one at startup, twice
/// under React StrictMode's double boot) added another handler and each menu
/// click fired N times — one ⌘V pasted an image three times.
pub fn setup(app: &AppHandle, lang: &str) -> tauri::Result<()> {
    let t = i18n::l(lang);

    let about = AboutMetadataBuilder::new()
        .name(Some("Harbly"))
        .version(Some("0.1.0"))
        .comments(Some(t.about_comment))
        .build();

    let app_menu = SubmenuBuilder::new(app, "Harbly")
        .about(Some(about))
        .separator()
        .item(
            &MenuItemBuilder::with_id("settings", t.settings)
                .accelerator("CmdOrCtrl+,")
                .build(app)?,
        )
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file = SubmenuBuilder::new(app, t.menu_file)
        .item(
            &MenuItemBuilder::with_id("new-md", t.new_md)
                .accelerator("CmdOrCtrl+N")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("new-hdoc", t.new_hdoc)
                .accelerator("CmdOrCtrl+Alt+N")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("import", t.import_html)
                .accelerator("CmdOrCtrl+O")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("new-folder", t.new_folder)
                .accelerator("CmdOrCtrl+Shift+N")
                .build(app)?,
        )
        .separator()
        // Native layer takes over ⌘⌫ (Finder: File > Move to Trash); inside input
        // fields the frontend forwards it back as "delete to line start"
        .item(
            &MenuItemBuilder::with_id("trash", t.trash)
                .accelerator("CmdOrCtrl+Backspace")
                .build(app)?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id("reveal-library", t.reveal_library).build(app)?)
        .build()?;

    let edit = SubmenuBuilder::new(app, t.menu_edit)
        .item(
            &MenuItemBuilder::with_id("undo", t.undo)
                .accelerator("CmdOrCtrl+Z")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("redo", t.redo)
                .accelerator("CmdOrCtrl+Shift+Z")
                .build(app)?,
        )
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(t.cut))?)
        .item(
            &MenuItemBuilder::with_id("copy", t.copy)
                .accelerator("CmdOrCtrl+C")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("paste", t.paste)
                .accelerator("CmdOrCtrl+V")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("paste-move", t.paste_move)
                .accelerator("CmdOrCtrl+Alt+V")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("select-all", t.select_all)
                .accelerator("CmdOrCtrl+A")
                .build(app)?,
        )
        .build()?;

    let view = SubmenuBuilder::new(app, t.menu_view)
        .item(
            &MenuItemBuilder::with_id("search", t.search)
                .accelerator("CmdOrCtrl+K")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("toggle-sidebar", t.toggle_sidebar)
                .accelerator("CmdOrCtrl+B")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("rescan", t.rescan)
                .accelerator("CmdOrCtrl+R")
                .build(app)?,
        )
        .build()?;

    let window = SubmenuBuilder::new(app, t.menu_window)
        .item(&PredefinedMenuItem::minimize(app, Some(t.minimize))?)
        .separator()
        .item(&PredefinedMenuItem::close_window(
            app,
            Some(t.close_window),
        )?)
        .build()?;

    let menu = Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window])?;
    app.set_menu(menu)?;
    Ok(())
}

/// Forward menu clicks to the frontend. Attached exactly once at startup —
/// never from setup(), which set_language re-runs (see above).
pub fn attach_event_bridge(app: &AppHandle) {
    app.on_menu_event(|app, event| {
        let _ = app.emit("menu-action", event.id().0.clone());
    });
}
