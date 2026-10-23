import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState, type JSX } from "react";

import { cn } from "@/lib/utils";

/** Native window operations in Prism's own title bar. */
export function WindowControls(): JSX.Element {
  const [maximized, setMaximized] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const window = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const read = async (): Promise<void> => {
      try {
        const value = await window.isMaximized();
        if (!disposed) setMaximized(value);
      } catch (cause) {
        if (!disposed)
          setError(cause instanceof Error ? cause.message : "Window state unavailable");
      }
    };
    void read();
    void window
      .onResized(() => {
        void read();
      })
      .then((release) => {
        if (disposed) release();
        else unlisten = release;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const run = (operation: () => Promise<void>): void => {
    setError(null);
    void operation().catch((cause: unknown) => {
      setError(cause instanceof Error ? cause.message : "The window operation failed");
    });
  };

  return (
    <div className="flex items-center gap-0.5" aria-label="Window controls">
      {error !== null ? (
        <span className="sr-only" role="alert">
          {error}
        </span>
      ) : null}
      <Control
        label="Minimize"
        onClick={() => {
          run(() => getCurrentWindow().minimize());
        }}
      >
        <Minus className="h-4 w-4" aria-hidden="true" />
      </Control>
      <Control
        label={maximized ? "Restore window" : "Maximize"}
        onClick={() => {
          run(async () => {
            await getCurrentWindow().toggleMaximize();
            setMaximized(await getCurrentWindow().isMaximized());
          });
        }}
      >
        {maximized ? (
          <Copy className="h-3.5 w-3.5" aria-hidden="true" />
        ) : (
          <Square className="h-3.5 w-3.5" aria-hidden="true" />
        )}
      </Control>
      <Control
        label="Close"
        danger
        onClick={() => {
          run(() => getCurrentWindow().close());
        }}
      >
        <X className="h-4 w-4" aria-hidden="true" />
      </Control>
    </div>
  );
}

function Control({
  label,
  onClick,
  danger = false,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: JSX.Element;
}): JSX.Element {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "grid h-8 w-9 place-items-center rounded-lg text-white/40 transition-colors",
        "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
        danger
          ? "hover:bg-critical/15 hover:text-critical"
          : "hover:bg-white/[0.07] hover:text-white",
      )}
    >
      {children}
    </button>
  );
}
