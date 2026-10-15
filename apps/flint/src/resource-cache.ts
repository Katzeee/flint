type Request<T> = {
  controller: AbortController;
  promise: Promise<T>;
};

export type ResourceEntry<T> = {
  data: T | null;
  updatedAt: number;
  users: number;
  version: number;
  request?: Request<T>;
};

// Only read results live here. Pages and their effects still unmount when navigation leaves them.
export class ResourceCache {
  private entries = new Map<string, ResourceEntry<unknown>>();
  private limit: number;

  constructor(limit = 64) {
    this.limit = limit;
  }

  peek<T>(key: string): ResourceEntry<T> | undefined {
    return this.entries.get(key) as ResourceEntry<T> | undefined;
  }

  acquire<T>(key: string): ResourceEntry<T> {
    const entry = this.peek<T>(key) ?? { data: null, updatedAt: 0, users: 0, version: 0 };
    entry.users += 1;
    this.entries.delete(key);
    this.entries.set(key, entry);
    this.trim();
    return entry;
  }

  release<T>(entry: ResourceEntry<T>) {
    entry.users -= 1;
    if (entry.users === 0) entry.request?.controller.abort();
    this.trim();
  }

  read<T>(entry: ResourceEntry<T>, reader: (signal: AbortSignal) => Promise<T>): Promise<T> {
    if (entry.request) return entry.request.promise;
    const controller = new AbortController();
    const version = entry.version;
    const promise = Promise.resolve()
      .then(() => {
        controller.signal.throwIfAborted();
        return reader(controller.signal);
      })
      .then((data) => {
        // A read that started before a write may describe the state that write replaced.
        if (entry.version !== version) return entry.data as T;
        // An IPC read already executing may finish after release. Keep that result for this key,
        // without publishing it to a page that has left or starting another identical native job.
        entry.data = data;
        entry.updatedAt = Date.now();
        return data;
      })
      .finally(() => {
        if (entry.request?.promise === promise) entry.request = undefined;
        this.trim();
      });
    entry.request = { controller, promise };
    return promise;
  }

  // Later reads start after the write instead of joining a read it outdated.
  write<T>(entry: ResourceEntry<T>, update: (data: T | null) => T): T {
    const data = update(entry.data);
    entry.data = data;
    entry.updatedAt = Date.now();
    entry.version += 1;
    entry.request = undefined;
    return data;
  }

  private trim() {
    for (const [key, entry] of this.entries) {
      if (this.entries.size <= this.limit) break;
      if (entry.users === 0 && !entry.request) this.entries.delete(key);
    }
  }
}
