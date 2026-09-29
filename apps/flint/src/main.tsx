import "@cairn/ui/styles.css";
import "@cairn/ui/themes/forest.css";
import "@cairn/host-tauri/styles.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app.js";

const root = document.querySelector("#root");
if (root === null) throw new Error("Flint UI root is missing");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
