import type { HostKind } from "./generated/host.js";

export type Failure = Readonly<{
  code: string;
  message: string;
}>;

export type BackendStatus = Readonly<{
  ready: boolean;
  pid: number;
  bridge_address: string;
  bridge_port: number;
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
  host: HostKind;
  pid: number;
  executable: string;
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
  error: Failure | null;
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
  bridge_endpoint: string;
  state_dir: string;
  attach_supported: boolean;
}>;

export type Snapshot = Readonly<{
  backend: BackendStatus | null;
  instances: readonly ConnectedInstance[];
}>;

declare global {
  interface Window {
    __TAURI__?: {
      core: {
        invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
      };
      event: {
        listen(event: string, handler: () => void): Promise<() => void>;
      };
    };
  }
}

export class FailureError extends Error {
  readonly code: string;

  constructor({ code, message }: Failure) {
    super(message);
    this.name = "FailureError";
    this.code = code;
  }
}

function isFailure(value: unknown): value is Failure {
  return typeof value === "object" && value !== null &&
    "code" in value && typeof value.code === "string" &&
    "message" in value && typeof value.message === "string";
}

// Commands reject with a serialized Failure; callers receive it as an Error.
async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const api = window.__TAURI__?.core;
  if (api === undefined) {
    throw new Error("The Flint desktop bridge is unavailable");
  }
  try {
    return await api.invoke<T>(command, args);
  } catch (error) {
    throw isFailure(error) ? new FailureError(error) : error;
  }
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

export type HostInfo = HostCandidate & Readonly<{
  window: Readonly<{ title: string; minimized: boolean }> | null;
  preview?: Readonly<{ image: string } | { unavailable_reason: string }>;
}>;

export function readHostInfo(pid: number, preview = false): Promise<HostInfo> {
  return invoke("host_info", { pid, preview });
}

export function focusApplication(pid: number): Promise<void> {
  return invoke("focus_application", { pid });
}

export type AttachResult = Readonly<{
  pid: number;
  host: HostKind;
  instance_id: string;
  execution_ready: boolean;
}>;

export function attachHost(
  pid: number,
  hostKind?: HostKind,
): Promise<AttachResult> {
  return invoke<AttachResult>("attach", { pid, hostKind });
}

// Launching flint while the desktop runs reopens this window instead of starting another desktop.
export function onDesktopOpened(handler: () => void): () => void {
  const api = window.__TAURI__?.event;
  if (api === undefined) {
    throw new Error("The Flint desktop bridge is unavailable");
  }
  const unlisten = api.listen("desktop-opened", handler);
  return () => void unlisten.then((stop) => stop());
}

export function activateTitleBar(): Promise<"custom" | "native"> {
  return invoke<"custom" | "native">("activate_title_bar");
}

let previousSnapshot: Snapshot | undefined;

export async function readSnapshot(): Promise<Snapshot> {
  const next = await invoke<Snapshot>("snapshot");
  // Unchanged global status must not rerender every page on each heartbeat poll.
  if (previousSnapshot && JSON.stringify(previousSnapshot) === JSON.stringify(next)) return previousSnapshot;
  previousSnapshot = next;
  return next;
}

export async function discoverHosts(): Promise<readonly HostCandidate[]> {
  return invoke("candidates");
}

export function startBackend(): Promise<BackendStatus> {
  return invoke("start_backend");
}

export function stopBackend(): Promise<Readonly<{
  stopped_pid: number | null;
}>> {
  return invoke("stop_backend");
}

export function restartBackend(): Promise<BackendStatus> {
  return invoke("restart_backend");
}
