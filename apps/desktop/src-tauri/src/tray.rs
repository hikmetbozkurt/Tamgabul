//! Notification-area icon with the docs/09 menu. Labels follow the UI language.

use tamgabul_core::settings::Language;
use tamgabul_platform::TrayCommand;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::windows;

const TRAY_ID: &str = "main";

fn label(cmd: TrayCommand, lang: Language) -> &'static str {
    match (cmd, lang) {
        (TrayCommand::OpenWidget, Language::En) => "Open Widget",
        (TrayCommand::OpenWidget, Language::Tr) => "Widget'ı aç",
        (TrayCommand::Settings, Language::En) => "Settings",
        (TrayCommand::Settings, Language::Tr) => "Ayarlar",
        (TrayCommand::ToggleIndexing, Language::En) => "Pause/Resume indexing",
        (TrayCommand::ToggleIndexing, Language::Tr) => "İndekslemeyi duraklat/sürdür",
        (TrayCommand::UnloadModels, Language::En) => "Unload Models",
        (TrayCommand::UnloadModels, Language::Tr) => "Modelleri boşalt",
        (TrayCommand::Quit, Language::En) => "Quit",
        (TrayCommand::Quit, Language::Tr) => "Çıkış",
    }
}

fn id(cmd: TrayCommand) -> &'static str {
    match cmd {
        TrayCommand::OpenWidget => "open_widget",
        TrayCommand::Settings => "settings",
        TrayCommand::ToggleIndexing => "toggle_indexing",
        TrayCommand::UnloadModels => "unload_models",
        TrayCommand::Quit => "quit",
    }
}

fn build_menu(app: &AppHandle, lang: Language) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for cmd in TrayCommand::MENU_ORDER {
        if cmd == TrayCommand::Quit {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        menu.append(&MenuItem::with_id(
            app,
            id(cmd),
            label(cmd, lang),
            true,
            None::<&str>,
        )?)?;
    }
    Ok(menu)
}

pub fn install(app: &AppHandle, lang: Language) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Tamgabul")
        .menu(&build_menu(app, lang)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_widget" => windows::show_widget(app),
            "settings" => windows::show_app(app, Some("settings".into())),
            "unload_models" => {
                // Stopping a worker waits for its process to exit: off the UI thread.
                let app = app.clone();
                std::thread::spawn(move || {
                    if let Some(state) = app.try_state::<crate::AppState>() {
                        state.models.unload_all();
                    }
                    let _ = app.emit("tray_action", "unload_models".to_string());
                });
            }
            // Indexing arrives in P1; the UI reacts to the event.
            "toggle_indexing" => {
                let _ = app.emit("tray_action", event.id.as_ref().to_string());
            }
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                windows::toggle_widget(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn relabel(app: &AppHandle, lang: Language) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app, lang)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Real exit: stop model workers (and wait for their processes) first.
pub fn quit(app: &AppHandle) {
    crate::hotkey::unregister_all(app);
    crate::stop_models(app);
    app.exit(0);
}
