//! Desktop IPC commands and the registry used by both Tauri and binding generation.
use crate::application::{
    App, Application, ApplicationInfo, BackendStopped, Failure, GetAppInfoResponse, HostKind, PingResponse, Snapshot,
    Workflow, WorkflowSummary,
};
use strum::IntoEnumIterator;
use tauri_plugin_decoration::WebviewWindowExt;

#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
enum TitleBarMode {
    Custom,
    Native,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub(super) struct DesktopOpened;

#[tauri::command]
#[specta::specta]
async fn app_info(
    state: tauri::State<'_, Application>,
    pid: u32,
    preview: bool,
) -> Result<GetAppInfoResponse, Failure> {
    state.app_info(pid, preview).await
}

#[tauri::command]
#[specta::specta]
async fn focus_app(state: tauri::State<'_, Application>, pid: u32) -> Result<(), Failure> {
    state.focus_app(pid).await
}

// A failed activation restores the native frame before it resolves, so either mode leaves a usable window.
#[tauri::command]
#[specta::specta]
async fn activate_title_bar(window: tauri::WebviewWindow) -> TitleBarMode {
    if let Err(error) = window.activate_decoration().await {
        eprintln!("custom title bar unavailable: {error}");
        return TitleBarMode::Native;
    }
    #[cfg(target_os = "macos")]
    if let Err(error) = window.set_traffic_lights_inset(16.0, 12.0).await {
        eprintln!("custom title bar unavailable: {error}");
        let _ = window.restore_decoration().await;
        return TitleBarMode::Native;
    }
    TitleBarMode::Custom
}

#[tauri::command]
#[specta::specta]
async fn snapshot(state: tauri::State<'_, Application>) -> Result<Snapshot, Failure> {
    state.snapshot().await
}

#[tauri::command]
#[specta::specta]
async fn attach(state: tauri::State<'_, Application>, pid: u32) -> Result<App, Failure> {
    state.attach(pid, None, None).await
}

#[tauri::command]
#[specta::specta]
async fn workflows(state: tauri::State<'_, Application>) -> Result<Vec<WorkflowSummary>, Failure> {
    state.workflows().await
}

#[tauri::command]
#[specta::specta]
async fn workflow(state: tauri::State<'_, Application>, id: String) -> Result<Workflow, Failure> {
    state.workflow(id).await
}

#[tauri::command]
#[specta::specta]
fn desktop_info(state: tauri::State<'_, Application>) -> ApplicationInfo {
    state.info()
}

#[tauri::command]
#[specta::specta]
async fn bridge_token(state: tauri::State<'_, Application>) -> Result<String, Failure> {
    state.bridge_token().await
}

#[tauri::command]
#[specta::specta]
async fn start_backend(state: tauri::State<'_, Application>) -> Result<PingResponse, Failure> {
    state.start_backend().await
}

#[tauri::command]
#[specta::specta]
async fn stop_backend(state: tauri::State<'_, Application>) -> Result<BackendStopped, Failure> {
    state.stop_backend().await
}

/// The command and event catalog shared by the desktop and `cargo codegen`.
pub(super) fn bindings() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .commands(tauri_specta::collect_commands![
            activate_title_bar,
            snapshot,
            attach,
            workflows,
            workflow,
            desktop_info,
            bridge_token,
            app_info,
            focus_app,
            start_backend,
            stop_backend
        ])
        .events(tauri_specta::collect_events![DesktopOpened])
        .constant(
            "hostNames",
            HostKind::iter()
                .map(|host| (<&'static str>::from(host), host.display_name()))
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        // Workflow counts cross JSON as numbers, matching their Serde representation.
        .dangerously_cast_bigints_to_number()
}
