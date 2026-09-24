export type BackendStatus = Readonly<{
  ready: boolean;
  pid: number;
  registry_host: string;
  registry_port: number;
}>;

export type ConnectedInstance = Readonly<{
  instance_name: string;
  instance_type: string;
  pid: number;
  runtime_version: string;
  execution_ready: boolean;
}>;

export type HostCandidate = Readonly<{
  host: string;
  pid: number;
}>;

export type Snapshot = Readonly<{
  backend: BackendStatus;
  instances: readonly ConnectedInstance[];
}>;

declare global {
  interface Window {
    __TAURI__?: { core: { invoke<T>(command: string): Promise<T> } };
  }
}

function invoke<T>(command: string): Promise<T> {
  const api = window.__TAURI__?.core;
  if (api === undefined) {
    return Promise.reject(new Error("The Flint desktop bridge is unavailable"));
  }
  return api.invoke<T>(command);
}

export function readSnapshot(): Promise<Snapshot> {
  return invoke<Snapshot>("snapshot");
}

export async function discoverHosts(): Promise<readonly HostCandidate[]> {
  const response = await invoke<{ hosts: readonly HostCandidate[] }>("candidates");
  return response.hosts;
}

export function stopBackend(): Promise<void> {
  return invoke<void>("stop_backend");
}
