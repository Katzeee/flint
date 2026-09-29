import { useCallback, useEffect, useRef, useState } from "react";

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
    key: null,
    data: null,
    error: "",
    loading: true,
  });
  useEffect(() => {
    if (key === null) return;
    let active = true;
    const controller = new AbortController();
    let timer: number | undefined;
    setResult((previous) => ({
      key,
      data: previous.key === key ? previous.data : null,
      error: "",
      loading: true,
    }));
    const refresh = async () => {
      try {
        const data = await reader.current(controller.signal);
        if (active) setResult({ key, data, error: "", loading: false });
      } catch (error) {
        if (active)
          setResult((previous) => ({
            ...previous,
            key,
            error: messageOf(error),
            loading: false,
          }));
      } finally {
        if (active && interval > 0)
          timer = window.setTimeout(refresh, interval);
      }
    };
    void refresh();
    return () => {
      active = false;
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [key, interval, revision, refreshVersion]);
  const reload = useCallback(() => setRevision((value) => value + 1), []);
  return {
    data: result.key === key ? result.data : null,
    error: result.key === key ? result.error : "",
    loading: key !== null && (result.key !== key || result.loading),
    reload,
  };
}
