//! Desktop IPC commands and the registry used by both Tauri and binding generation.
use crate::application::{
    Application, ApplicationInfo, AttachResult, BackendStopped, Failure, GetWorkflowResponse,
    HostCandidate, HostInfo, HostKind, PingResponse, Snapshot, WorkflowSummary,
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
async fn host_info(pid: u32, preview: bool) -> Result<HostInfo, Failure> {
    Application::host_info(pid, preview).await
}

#[tauri::command]
#[specta::specta]
async fn focus_application(pid: u32) -> Result<(), Failure> {
    Application::focus_application(pid).await
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
async fn candidates() -> Result<Vec<HostCandidate>, Failure> {
    Application::hosts().await
}

#[tauri::command]
#[specta::specta]
async fn attach(
    state: tauri::State<'_, Application>,
    pid: u32,
    host_kind: Option<HostKind>,
) -> Result<AttachResult, Failure> {
    state.attach(pid, host_kind, None).await
}

#[tauri::command]
#[specta::specta]
async fn workflows(state: tauri::State<'_, Application>) -> Result<Vec<WorkflowSummary>, Failure> {
    state.workflows().await
}

#[tauri::command]
#[specta::specta]
async fn workflow(
    state: tauri::State<'_, Application>,
    id: String,
) -> Result<GetWorkflowResponse, Failure> {
    state.workflow(id).await
}

#[tauri::command]
#[specta::specta]
fn desktop_info(state: tauri::State<'_, Application>) -> ApplicationInfo {
    state.info()
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

#[tauri::command]
#[specta::specta]
async fn restart_backend(state: tauri::State<'_, Application>) -> Result<PingResponse, Failure> {
    state.restart_backend().await
}

/// The command and event catalog shared by the desktop and `cargo codegen`.
pub(super) fn bindings() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .commands(tauri_specta::collect_commands![
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
        .events(tauri_specta::collect_events![DesktopOpened])
        .constant("hostKinds", HostKind::iter().collect::<Vec<_>>())
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        // Workflow counts cross JSON as numbers, matching their Serde representation.
        .dangerously_cast_bigints_to_number()
}
