import { commands, events, type Failure, type HostKind, type Snapshot } from "./generated/bindings.js";

export type {
  Failure,
  Snapshot,
  HostCandidate,
  HostInfo_Serialize as HostInfo,
  InstanceInfo as ConnectedInstance,
  GetExecutionResponse_Serialize as Execution,
  GetWorkflowResponse_Serialize as Workflow,
  WorkflowSummary,
} from "./generated/bindings.js";

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

// Generated commands preserve Tauri rejections; views receive diagnostic Errors.
async function withFailure<T>(result: Promise<T>): Promise<T> {
  try {
    return await result;
  } catch (error) {
    throw isFailure(error) ? new FailureError(error) : error;
  }
}

export function readWorkflows() {
  return withFailure(commands.workflows());
}

export function readWorkflow(id: string) {
  return withFailure(commands.workflow(id));
}

export function readDesktopInfo() {
  return withFailure(commands.desktopInfo());
}

export function readHostInfo(pid: number, preview = false) {
  return withFailure(commands.hostInfo(pid, preview));
}

export function focusApplication(pid: number) {
  return withFailure(commands.focusApplication(pid));
}

export function attachHost(pid: number, hostKind?: HostKind) {
  return withFailure(commands.attach(pid, hostKind ?? null));
}

// Launching flint while the desktop runs reopens this window instead of starting another desktop.
export function onDesktopOpened(handler: () => void): () => void {
  const unlisten = events.desktopOpened.listen(handler);
  return () => void unlisten.then((stop) => stop());
}

export function activateTitleBar() {
  return withFailure(commands.activateTitleBar());
}

let previousSnapshot: Snapshot | undefined;

export async function readSnapshot(): Promise<Snapshot> {
  const next = await withFailure(commands.snapshot());
  // Unchanged global status must not rerender every page on each heartbeat poll.
  if (previousSnapshot && JSON.stringify(previousSnapshot) === JSON.stringify(next)) return previousSnapshot;
  previousSnapshot = next;
  return next;
}

export function discoverHosts() {
  return withFailure(commands.candidates());
}

export function startBackend() {
  return withFailure(commands.startBackend());
}

export function stopBackend() {
  return withFailure(commands.stopBackend());
}

export function restartBackend() {
  return withFailure(commands.restartBackend());
}
