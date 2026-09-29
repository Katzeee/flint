import { useEffect, useState } from "react";

export type Route = Readonly<{
  page: "apps" | "workflows" | "settings" | "legal";
  id?: string;
  candidate?: boolean;
}>;

function readRoute(): Route {
  const [page, ...segments] = window.location.hash
    .replace(/^#\/?/, "")
    .split("/");
  try {
    if (page === "workflows")
      return {
        page,
        id: segments[0] ? decodeURIComponent(segments[0]) : undefined,
      };
    if (page === "settings" || page === "legal") return { page };
    return {
      page: "apps",
      id:
        segments[0] === "candidates"
          ? segments[1]
          : segments[0]
            ? decodeURIComponent(segments[0])
            : undefined,
      candidate: segments[0] === "candidates",
    };
  } catch {
    return { page: "apps" };
  }
}

export function useRoute(): Route {
  const [route, setRoute] = useState(readRoute);
  useEffect(() => {
    const update = () => setRoute(readRoute());
    window.addEventListener("hashchange", update);
    return () => window.removeEventListener("hashchange", update);
  }, []);
  return route;
}

export function navigate(path: string) {
  window.location.hash = `#/${path}`;
}

export function workflowPath(id: string) {
  return `workflows/${encodeURIComponent(id)}`;
}
export function instancePath(id: string) {
  return `apps/${encodeURIComponent(id)}`;
}
