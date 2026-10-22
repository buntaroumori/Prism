import type { JSX } from "react";

import logoUrl from "../../src-tauri/icons/128x128.png";
import { cn } from "@/lib/utils";

interface AppLogoProps {
  className?: string;
}

/** The same Prism mark used by the installed application icon. */
export function AppLogo({ className }: AppLogoProps): JSX.Element {
  return (
    <img
      src={logoUrl}
      alt="Prism"
      draggable={false}
      className={cn("h-9 w-9 shrink-0 rounded-lg object-cover", className)}
    />
  );
}
