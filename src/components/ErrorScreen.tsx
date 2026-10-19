import type { JSX } from "react";
import { RefreshCw, Terminal, WifiOff } from "lucide-react";

import { AppLogo } from "@/components/AppLogo";
import { Button } from "@/components/ui/Button";
import type { UiError } from "@/lib/errors";

interface ErrorScreenProps {
  error: UiError;
  refreshing: boolean;
  onRetry: () => void;
}

/**
 * Boot failure surface.
 *
 * A `transport` code means the WebView is running outside the desktop shell
 * (e.g. `vite preview` in a browser) or the IPC bridge did not initialise;
 * any other code is a genuine backend failure with its message shown verbatim.
 */
export function ErrorScreen({ error, refreshing, onRetry }: ErrorScreenProps): JSX.Element {
  const isTransport = error.code === "transport";

  return (
    <div className="flex min-h-[70vh] items-center justify-center">
      <div className="glass w-full max-w-xl rounded-4xl p-10 text-center">
        <AppLogo className="mx-auto h-12 w-12" />

        <div className="mt-6 inline-flex items-center gap-2 rounded-full border border-critical/25 bg-critical/[0.08] px-3 py-1 text-[10px] font-medium tracking-[0.12em] text-critical uppercase">
          <WifiOff className="h-3.5 w-3.5" aria-hidden="true" />
          {isTransport ? "Desktop bridge unavailable" : "Backend failure"}
        </div>

        <h1 className="mt-5 text-2xl font-semibold tracking-tight text-white">
          {isTransport ? "Prism could not reach its Rust core." : "Prism could not start."}
        </h1>

        <p className="mx-auto mt-3 max-w-md text-[13px] leading-relaxed text-white/55">
          {error.message}
        </p>

        {isTransport ? (
          <div className="mt-6 rounded-2xl border border-white/[0.07] bg-black/25 p-4 text-left">
            <p className="flex items-center gap-2 text-[11px] font-medium tracking-wide text-white/45 uppercase">
              <Terminal className="h-3.5 w-3.5" aria-hidden="true" />
              Expected environment
            </p>
            <p className="mt-2 text-[12px] leading-relaxed text-white/40">
              This interface talks to the Tauri backend over its IPC bridge. Run the desktop shell
              with{" "}
              <code className="rounded bg-white/[0.07] px-1.5 py-0.5 font-mono">
                pnpm tauri dev
              </code>{" "}
              instead of opening the Vite URL in a plain browser.
            </p>
          </div>
        ) : null}

        <div className="mt-7 flex flex-wrap items-center justify-center gap-2">
          <Button variant="primary" icon={RefreshCw} loading={refreshing} onClick={onRetry}>
            Try again
          </Button>
        </div>

        <p className="mt-5 font-mono text-[10px] text-white/20">error code · {error.code}</p>
      </div>
    </div>
  );
}
