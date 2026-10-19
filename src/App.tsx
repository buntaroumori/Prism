import type { JSX } from "react";

import { AppShell } from "@/components/AppShell";
import { DesktopRequired } from "@/components/DesktopRequired";
import { hasBackend } from "@/lib/cover";

/**
 * Root entry.
 *
 * Two states only: the desktop shell, or a clear explanation that this bundle is
 * being viewed outside the Tauri runtime. The error boundary and the OLED boot
 * paint live in `main.tsx` and `index.html` respectively, so this file stays a
 * routing concern.
 */
export function App(): JSX.Element {
  return hasBackend() ? <AppShell /> : <DesktopRequired />;
}
