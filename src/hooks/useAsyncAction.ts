import { useCallback, useEffect, useRef, useState } from "react";

import { toUiError, type UiError } from "@/lib/errors";

export interface AsyncAction {
  isRunning: boolean;
  error: UiError | null;
  run: (action: () => Promise<void>) => void;
  clearError: () => void;
}

/**
 * Run a user-triggered async action exactly once at a time.
 *
 * Guarantees:
 *   * the button can key off `isRunning` without re-entrancy bugs;
 *   * failures are converted into a `UiError` instead of an unhandled
 *     rejection (which WebKit logs as a renderer-level error);
 *   * no state is committed after the owning component unmounts.
 */
export function useAsyncAction(): AsyncAction {
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const run = useCallback((action: () => Promise<void>) => {
    setIsRunning(true);
    setError(null);

    void (async () => {
      try {
        await action();
      } catch (cause) {
        if (mounted.current) {
          setError(toUiError(cause));
        }
      } finally {
        if (mounted.current) {
          setIsRunning(false);
        }
      }
    })();
  }, []);

  const clearError = useCallback(() => {
    setError(null);
  }, []);

  return { isRunning, error, run, clearError };
}
