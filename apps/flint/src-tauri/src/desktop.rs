use flint_backend::{Backend, BackendHandle};
use flint_control_client::{instance_json, status_json};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_decoration::WebviewWindowExt;

#[tauri::command]
async fn host_info(pid: u32, preview: bool) -> Result<serde_json::Value, String> {
    flint_hosts::host_info(pid, preview)
        .await
        .map(crate::hosts::host_info_json)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn focus_application(pid: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || flint_hosts::focus_application(pid))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

// A failed activation restores the native frame before it resolves, so either mode leaves a usable window.
#[tauri::command]
async fn activate_title_bar(window: tauri::WebviewWindow) -> &'static str {
    if let Err(error) = window.activate_decoration().await {
        eprintln!("Custom title bar unavailable: {error}");
        return "native";
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = window.set_traffic_lights_inset(16.0, 12.0).await {
        eprintln!("Custom title bar unavailable: {error}");
        let _ = window.restore_decoration().await;
        return "native";
    }
    "custom"
}

#[tauri::command]
fn snapshot(state: tauri::State<'_, BackendHandle>) -> serde_json::Value {
    serde_json::json!({"backend":status_json(state.status()),"instances":state.instances().into_iter().map(instance_json).collect::<Vec<_>>()})
}
#[tauri::command]
async fn candidates() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| serde_json::json!({"hosts": flint_hosts::discover()}))
        .await
        .map_err(|error| error.to_string())
}
#[tauri::command]
async fn workflows(state: tauri::State<'_, BackendHandle>) -> Result<serde_json::Value, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        backend
            .workflows()
            .map(|items| serde_json::json!(items))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn workflow(
    state: tauri::State<'_, BackendHandle>,
    id: String,
) -> Result<serde_json::Value, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        backend
            .workflow(&id)
            .map(|item| serde_json::json!(item))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn desktop_info(state: tauri::State<'_, BackendHandle>) -> serde_json::Value {
    let config = state.config();
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "control_endpoint": format!("{}:{}", config.host, config.port),
        "registry_endpoint": format!("{}:{}", config.registry_host, config.registry_port),
        "state_dir": config.state_dir,
    })
}
#[tauri::command]
fn stop_backend(state: tauri::State<'_, BackendHandle>) -> Result<(), String> {
    state.request_stop().map_err(|e| e.to_string())
}

pub fn run(backend: Backend, runtime: tokio::runtime::Runtime) -> anyhow::Result<()> {
    let handle = backend.handle();
    let notifications = handle.window_notifications();
    let shutdown = handle.shutdown_token();
    let menu_handle = handle.clone();
    let mut backend = Some(backend);
    let mut context = tauri::generate_context!();
    // Keep window icons sharp regardless of the frame order inside the ICO.
    context.set_default_window_icon(Some(tauri::include_image!("icons/icon.png")));
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_decoration::init())
        .manage(handle)
        .invoke_handler(tauri::generate_handler![activate_title_bar, snapshot, candidates, workflows, workflow, desktop_info, host_info, focus_application, stop_backend])
        .setup(move |app| {
            let show = MenuItem::with_id(app, "show", "Open flint", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Stop flint", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show,&quit])?;
            TrayIconBuilder::with_id("flint")
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("flint · backend running")
                .menu(&menu)
                .on_menu_event(move |app,event| match event.id.as_ref() {
                    "show" => if let Some(window)=app.get_webview_window("main") { let _=window.show(); let _=window.set_focus(); },
                    "quit" => if menu_handle.request_stop().is_err() { if let Some(window)=app.get_webview_window("main") { let _=window.show(); } },
                    _ => {},
                }).build(app)?;
            let app_handle = app.handle().clone();
            let task_app = app_handle.clone();
            let service = backend.take().unwrap();
            runtime.spawn(async move {
                let status = service.run().await;
                if let Err(ref error) = status { eprintln!("Backend stopped: {error:#}"); }
                task_app.exit(if status.is_ok(){0}else{1});
            });
            let watch_app = app_handle.clone();
            let state = app.state::<BackendHandle>().inner().clone();
            runtime.spawn(async move {
                loop {
                    tokio::select! {
                        _=shutdown.cancelled()=>break,
                        _=notifications.notified()=>if let Some(window)=watch_app.get_webview_window("main") { let _=window.show(); let _=window.set_focus(); },
                        _=tokio::time::sleep(std::time::Duration::from_secs(1))=>{
                            if let Some(tray)=watch_app.tray_by_id("flint") { let _=tray.set_tooltip(Some(format!("flint · {} connected hosts",state.instances().len()))); }
                        }
                    }
                }
            });
            // Keep the service runtime alive for the desktop application's lifetime.
            app.manage(runtime);
            Ok(())
        })
        .on_window_event(|window,event| {
            if let tauri::WindowEvent::CloseRequested{api,..}=event { api.prevent_close(); let _=window.hide(); }
        })
        .build(context)?;
    app.run(|_, _| {});
    Ok(())
}
