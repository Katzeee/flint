import { readWindowPreview, type WindowPreview } from "./backend.js";

// Match the desktop's capture capacity. Leaving a page removes its queued captures;
// captures already running keep their slot until the native call finishes.
const pending: (() => void)[] = [];
let running = 0;

function drain() {
  while (running < 2 && pending.length) pending.shift()!();
}

export function loadWindowPreview(pid: number, host: string, signal: AbortSignal) {
  return new Promise<WindowPreview>((resolve, reject) => {
    const cancel = () => {
      const index = pending.indexOf(start);
      if (index >= 0) pending.splice(index, 1);
      reject(new DOMException("Preview cancelled", "AbortError"));
    };
    const start = () => {
      signal.removeEventListener("abort", cancel);
      running += 1;
      void readWindowPreview(pid, host)
        .then(resolve, reject)
        .finally(() => {
          running -= 1;
          drain();
        });
    };
    if (signal.aborted) {
      cancel();
      return;
    }
    signal.addEventListener("abort", cancel, { once: true });
    pending.push(start);
    drain();
  });
}
