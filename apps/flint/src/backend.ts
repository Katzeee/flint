export type BackendStatus = Readonly<{
  ready: boolean;
  pid: number;
  registry_host: string;
  registry_port: number;
}>;

export type ConnectedInstance = Readonly<{
  instance_id: string;
  instance_name: string;
  instance_type: string;
  pid: number;
  runtime_version: string;
  bridge_version: string;
  execution_ready: boolean;
}>;

export type HostCandidate = Readonly<{
  host: string;
  pid: number;
  executable: string;
  attach_supported: boolean;
}>;

export type WorkflowSummary = Readonly<{
  workflow_id: string;
  name: string;
  description: string;
  updated_at: string;
  execution_count: number;
  instance_ids: readonly string[];
  running_count: number;
  failed_count: number;
}>;

export type Execution = Readonly<{
  execution_id: string;
  name: string;
  instance_id: string;
  code: string;
  status: string;
  stdout: string;
  stderr: string;
  started_at: string;
  finished_at: string | null;
  traceback: string | null;
  error: string | null;
}>;

export type Workflow = Readonly<{
  workflow_id: string;
  name: string;
  description: string;
  created_at: string;
  execs: readonly Execution[];
}>;

export type DesktopInfo = Readonly<{
  version: string;
  control_endpoint: string;
  registry_endpoint: string;
  state_dir: string;
}>;

export type Snapshot = Readonly<{
  backend: BackendStatus;
  instances: readonly ConnectedInstance[];
}>;

declare global {
  interface Window {
    __TAURI__?: {
      core: {
        invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
      };
    };
  }
}

function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const api = window.__TAURI__?.core;
  if (api === undefined) {
    return Promise.reject(new Error("The Flint desktop bridge is unavailable"));
  }
  return api.invoke<T>(command, args);
}

export function readWorkflows(): Promise<readonly WorkflowSummary[]> {
  return invoke("workflows");
}

export function readWorkflow(id: string): Promise<Workflow> {
  return invoke("workflow", { id });
}

export function readDesktopInfo(): Promise<DesktopInfo> {
  return invoke("desktop_info");
}

export type WindowPreview = Readonly<{
  title: string;
  image: string | null;
  unavailable_reason: string | null;
  can_focus: boolean;
}>;

export function readWindowPreview(instanceId: string): Promise<WindowPreview> {
  return invoke("window_preview", { instanceId });
}

export function focusInstance(instanceId: string): Promise<void> {
  return invoke("focus_instance", { instanceId });
}

export function activateTitleBar(): Promise<"custom" | "native"> {
  return invoke<"custom" | "native">("activate_title_bar");
}

export function readSnapshot(): Promise<Snapshot> {
  return invoke<Snapshot>("snapshot");
}

export async function discoverHosts(): Promise<readonly HostCandidate[]> {
  const response = await invoke<{ hosts: readonly HostCandidate[] }>(
    "candidates",
  );
  return response.hosts;
}

export function stopBackend(): Promise<void> {
  return invoke<void>("stop_backend");
}
