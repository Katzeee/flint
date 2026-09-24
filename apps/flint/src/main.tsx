import "@cairn/ui/styles.css";
import "./style.css";

import { LegalPage } from "@cairn/ui";
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app.js";

const root = document.querySelector("#root");
if (root === null) {
  throw new Error("Flint UI root is missing");
}

function Root() {
  const [legal, setLegal] = useState(() => window.location.hash === "#/legal");
  useEffect(() => {
    const update = () => setLegal(window.location.hash === "#/legal");
    window.addEventListener("hashchange", update);
    return () => window.removeEventListener("hashchange", update);
  }, []);
  return legal ? (
    <LegalPage backHref="#/" backLabel="Back to Flint" />
  ) : (
    <App />
  );
}

createRoot(root).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
