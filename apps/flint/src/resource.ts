import { useCallback, useEffect, useRef, useState } from "react";
import { ResourceCache } from "./resource-cache.js";

const resources = new ResourceCache();

export function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// Each selection owns its response. An old request cannot overwrite a newly selected object's data.
export function useResource<T>(
  key: string | null,
  read: (signal: AbortSignal) => Promise<T>,
  interval = 0,
  refreshVersion = 0,
) {
  const cached = key === null ? undefined : resources.peek<T>(key);
  const freshness = interval || 30000;
  const previous = useRef({ key, refreshVersion, revision: 0 });
  const reader = useRef(read);
  useEffect(() => {
    reader.current = read;
  });
  const [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{
    key: string | null;
    data: T | null;
    error: string;
    loading: boolean;
  }>({
    key,
    data: cached?.data ?? null,
    error: "",
    loading: true,
  });
  useEffect(() => {
    if (key === null) return;
    const force =
      previous.current.key === key &&
      (previous.current.revision !== revision || previous.current.refreshVersion !== refreshVersion);
    previous.current = { key, refreshVersion, revision };
    const entry = resources.acquire<T>(key);
    let active = true;
    let timer: number | undefined;
    const remaining = freshness - (Date.now() - entry.updatedAt);
    const fresh = entry.data !== null && remaining > 0 && !force;
    setResult({
      key,
      data: entry.data,
      error: "",
      loading: !fresh,
    });
    const refresh = async () => {
      let cancelled = false;
      try {
        const data = await resources.read(entry, reader.current);
        if (active)
          setResult((previous) =>
            previous.key === key && previous.data === data && !previous.error && !previous.loading
              ? previous
              : { key, data, error: "", loading: false },
          );
      } catch (error) {
        cancelled = error instanceof DOMException && error.name === "AbortError";
        if (active && !cancelled)
          setResult((previous) => ({
            ...previous,
            key,
            error: messageOf(error),
            loading: false,
          }));
      } finally {
        if (active && (cancelled || interval > 0)) timer = window.setTimeout(refresh, cancelled ? 0 : interval);
      }
    };
    if (!fresh) void refresh();
    else if (interval > 0) timer = window.setTimeout(refresh, remaining);
    return () => {
      active = false;
      window.clearTimeout(timer);
      resources.release(entry);
    };
  }, [key, interval, revision, refreshVersion, freshness]);
  const reload = useCallback(() => setRevision((value) => value + 1), []);
  return {
    data: result.key === key ? result.data : (cached?.data ?? null),
    error: result.key === key ? result.error : "",
    loading: key !== null && (result.key !== key || result.loading),
    reload,
  };
}
