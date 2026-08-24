import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { AppShell } from "./app-shell.js";

const root = document.querySelector<HTMLElement>("#root");
if (root === null) {
  throw new Error("React root is missing");
}

createRoot(root).render(
  <StrictMode>
    <AppShell />
  </StrictMode>,
);
