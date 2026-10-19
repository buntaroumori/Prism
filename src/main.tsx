import "@fontsource-variable/inter";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "@/App";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import "@/styles/globals.css";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("Prism bootstrap failed: #root container is missing from index.html");
}

createRoot(container).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
