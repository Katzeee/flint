use crate::application::{
    Application, ApplicationInfo, AttachResult, BackendStopped, Failure, GetWorkflowResponse,
    HostCandidate, HostInfo, HostKind, PingResponse, Snapshot, WorkflowSummary,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tauri_plugin_decoration::WebviewWindowExt;

#[tauri::command]
async fn host_info(pid: u32, preview: bool) -> Result<HostInfo, Failure> {
    Application::host_info(pid, preview).await
}

#[tauri::command]
async fn focus_application(pid: u32) -> Result<(), Failure> {
    Application::focus_application(pid).await
}

// A failed activation restores the native frame before it resolves, so either mode leaves a usable window.
#[tauri::command]
async fn activate_title_bar(window: tauri::WebviewWindow) -> &'static str {
    if let Err(error) = window.activate_decoration().await {
        eprintln!("custom title bar unavailable: {error}");
        return "native";
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = window.set_traffic_lights_inset(16.0, 12.0).await {
        eprintln!("custom title bar unavailable: {error}");
        let _ = window.restore_decoration().await;
        return "native";
    }
    "custom"
}

#[tauri::command]
async fn snapshot(state: tauri::State<'_, Application>) -> Result<Snapshot, Failure> {
    state.snapshot().await
}

#[tauri::command]
async fn candidates() -> Result<Vec<HostCandidate>, Failure> {
    Application::hosts().await
}

#[tauri::command]
async fn attach(
    state: tauri::State<'_, Application>,
    pid: u32,
    host_kind: Option<HostKind>,
) -> Result<AttachResult, Failure> {
    state.attach(pid, host_kind, None).await
}

#[tauri::command]
async fn workflows(state: tauri::State<'_, Application>) -> Result<Vec<WorkflowSummary>, Failure> {
    state.workflows().await
}

#[tauri::command]
async fn workflow(
    state: tauri::State<'_, Application>,
    id: String,
) -> Result<GetWorkflowResponse, Failure> {
    state.workflow(id).await
}

#[tauri::command]
fn desktop_info(state: tauri::State<'_, Application>) -> ApplicationInfo {
    state.info()
}

#[tauri::command]
async fn start_backend(state: tauri::State<'_, Application>) -> Result<PingResponse, Failure> {
    state.start_backend().await
}

#[tauri::command]
async fn stop_backend(state: tauri::State<'_, Application>) -> Result<BackendStopped, Failure> {
    state.stop_backend().await
}

#[tauri::command]
async fn restart_backend(state: tauri::State<'_, Application>) -> Result<PingResponse, Failure> {
    state.restart_backend().await
}

fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn run(application: Application) -> anyhow::Result<()> {
    let mut context = tauri::generate_context!();
    // Each isolated product test has its own GUI instance and keeps its WebView data in the test root.
    #[cfg(feature = "test-runtime")]
    {
        let info = application.info();
        let config = context.config_mut();
        config.identifier = format!(
            "dev.flint.test.{}",
            info.control_endpoint.replace(['.', ':'], "-")
        );
        config.app.app_directories_override = Some(
            tauri::utils::config::AppDirectoriesOverride::Root(info.state_dir.join("desktop")),
        );
    }
    // Keep window icons sharp regardless of the frame order inside the ICO.
    context.set_default_window_icon(Some(tauri::include_image!("icons/icon.png")));
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show(app);
            let _ = app.emit("desktop-opened", ());
        }))
        .plugin(tauri_plugin_decoration::init())
        .manage(application)
        .invoke_handler(tauri::generate_handler![
            activate_title_bar,
            snapshot,
            candidates,
            attach,
            workflows,
            workflow,
            desktop_info,
            host_info,
            focus_application,
            start_backend,
            stop_backend,
            restart_backend
        ])
        .setup(|app| {
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
