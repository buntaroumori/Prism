import { Component, type ErrorInfo, type ReactNode } from "react";
import { RotateCcw, TriangleAlert } from "lucide-react";

import { AppLogo } from "@/components/AppLogo";
import { Button } from "@/components/ui/Button";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Last line of defence for render-time failures.
 *
 * React has no hook equivalent for `componentDidCatch`, so this is the one
 * class component in the codebase. It logs the component stack (never the raw
 * error object, which can contain paths) and offers a full reload, which
 * rebuilds the WebView state from the Rust side.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  override state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("Prism render failure:", error.message, info.componentStack);
  }

  private readonly handleReload = (): void => {
    window.location.reload();
  };

  override render(): ReactNode {
    const { error } = this.state;
    if (error === null) {
      return this.props.children;
    }

    return (
      <div className="relative flex h-full items-center justify-center bg-oled-950 p-10">
        <div className="glass w-full max-w-xl rounded-4xl p-10 text-center">
          <AppLogo className="mx-auto h-12 w-12" />
          <div className="mt-6 inline-flex items-center gap-2 rounded-full border border-critical/30 bg-critical/10 px-3 py-1 text-xs font-medium tracking-wide text-critical uppercase">
            <TriangleAlert className="h-3.5 w-3.5" aria-hidden="true" />
            Render failure
          </div>
          <h1 className="mt-5 text-2xl font-semibold tracking-tight text-white">
            The interface hit an unrecoverable error.
          </h1>
          <p className="mt-3 text-sm text-white/60">
            {error.message.length > 0
              ? error.message
              : "No diagnostic message was produced by the failing component."}
          </p>
          <p className="mt-2 text-xs text-white/40">
            Your library database was not modified. Reloading rebuilds the view from the Rust
            backend.
          </p>
          <Button className="mt-8" onClick={this.handleReload} icon={RotateCcw}>
            Reload Prism
          </Button>
        </div>
      </div>
    );
  }
}
