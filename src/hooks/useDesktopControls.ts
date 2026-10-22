import { useEffect } from "react";

import { useWindowStore } from "@/stores/windowStore";

const BLOCKED_CTRL_KEYS = new Set(["a", "f", "g", "j", "p"]);

/** Desktop input policy: no browser menus, find bars, print dialogs or Tab focus. */
export function useDesktopControls(): void {
  const restore = useWindowStore((state) => state.restore);

  useEffect(() => {
    void restore();
  }, [restore]);

  useEffect(() => {
    const contextMenu = (event: MouseEvent): void => {
      // Prevent only the browser default. React's application-owned handlers
      // still receive the event and open Prism's own menu on albums and songs.
      event.preventDefault();
    };
    const keyDown = (event: KeyboardEvent): void => {
      const key = event.key.toLowerCase();
      if (event.key === "F11") {
        event.preventDefault();
        event.stopPropagation();
        void useWindowStore.getState().toggleFullscreen();
        return;
      }
      if (event.key === "Tab" || event.key === "F12") {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (
        (event.ctrlKey || event.metaKey) &&
        (BLOCKED_CTRL_KEYS.has(key) || (event.shiftKey && ["i", "j", "c"].includes(key)))
      ) {
        event.preventDefault();
        event.stopPropagation();
      }
    };

    document.addEventListener("contextmenu", contextMenu, true);
    document.addEventListener("keydown", keyDown, true);
    return () => {
      document.removeEventListener("contextmenu", contextMenu, true);
      document.removeEventListener("keydown", keyDown, true);
    };
  }, []);
}
