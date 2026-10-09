use super::ipc::{self, DesktopOpened};
use crate::application::Application;
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_specta::Event;

fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn run(application: Application) -> anyhow::Result<()> {
    let mut context = tauri::generate_context!();
    // Each isolated product test has its own GUI instance and keeps its WebView data in the test root.
    #[cfg(feature = "test-runtime")]
    {
        let info = application.info();
        let config = context.config_mut();
        config.identifier = format!("dev.flint.test.{}", info.control_endpoint.replace(['.', ':'], "-"));
        config.app.app_directories_override = Some(tauri::utils::config::AppDirectoriesOverride::Root(
            info.state_dir.join("desktop"),
        ));
    }
    // Keep window icons sharp regardless of the frame order inside the ICO.
    context.set_default_window_icon(Some(tauri::include_image!("icons/icon.png")));
    let bindings = ipc::bindings();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show(app);
            let _ = DesktopOpened.emit(app);
        }))
        .plugin(tauri_plugin_decoration::init())
        .manage(application)
        .invoke_handler(bindings.invoke_handler())
        .setup(move |app| {
            bindings.mount_events(app);
            let open = MenuItem::with_id(app, "open", "Open flint", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit desktop", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("flint")
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("flint")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            show(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(context)?;
    app.run(|_, _| {});
    Ok(())
}
