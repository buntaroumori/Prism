import { useCallback, useEffect, useState } from "react";

import { toUiError, type UiError } from "@/lib/errors";
import { systemApi } from "@/lib/ipc";
import type { SystemSnapshot } from "@/types/ipc";

export interface SystemSnapshotResult {
  snapshot: SystemSnapshot | null;
  error: UiError | null;
  isLoading: boolean;
  refreshing: boolean;
  refresh: () => void;
}

/**
 * Load the boot snapshot (app info + runtime health + library counters).
 *
 * The effect is generation-guarded: a refresh triggered while the previous
 * request is in flight can never commit stale data, and unmounting cancels the
 * commit. Errors keep the last good snapshot visible so a transient failure
 * degrades to a banner instead of blanking the UI.
 */
export function useSystemSnapshot(): SystemSnapshotResult {
  const [snapshot, setSnapshot] = useState<SystemSnapshot | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    let active = true;

    const load = async (): Promise<void> => {
      try {
        const [appInfo, health, library] = await Promise.all([
          systemApi.getAppInfo(),
          systemApi.getRuntimeHealth(),
          systemApi.getLibraryStats(),
        ]);
        if (!active) {
          return;
        }
        setSnapshot({ appInfo, health, library, capturedAt: new Date() });
        setError(null);
      } catch (cause) {
        if (!active) {
          return;
        }
        setError(toUiError(cause));
      } finally {
        if (active) {
          setIsLoading(false);
          setRefreshing(false);
        }
      }
    };

    void load();

    return () => {
      active = false;
    };
  }, [reloadToken]);

  const refresh = useCallback(() => {
    setRefreshing(true);
    setReloadToken((token) => token + 1);
  }, []);

  return { snapshot, error, isLoading, refreshing, refresh };
}
