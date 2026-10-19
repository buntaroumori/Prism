import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { hasBackend } from "@/lib/cover";
import { EVENT_CHANNELS } from "@/lib/ipc";
import { useLibraryStore } from "@/stores/libraryStore";
import { ScanProgressSchema, ScanRunSchema } from "@/types/ipc";

/**
 * Subscribe the library store to the scanner's two channels.
 *
 * `library://scan-progress` arrives at most five times a second by design; the
 * listener validates it with the same schema the UI renders from, so a scan can
 * never push a malformed percentage into the progress bar. `scan-complete`
 * carries either an outcome or a `{code, message}` failure — both are accepted
 * and the outcome path refreshes the counters.
 */
export function useScanEvents(): void {
  useEffect(() => {
    if (!hasBackend()) {
      return;
    }

    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const attach = async (): Promise<void> => {
      const progressUnlisten = await listen<unknown>(EVENT_CHANNELS.scanProgress, (message) => {
        const parsed = ScanProgressSchema.safeParse(message.payload);
        if (!parsed.success) {
          return;
        }
        useLibraryStore.getState().applyProgress(parsed.data);
      });

      const completeUnlisten = await listen<unknown>(EVENT_CHANNELS.scanComplete, (message) => {
        const run = ScanRunSchema.safeParse(message.payload);
        if (run.success) {
          useLibraryStore.getState().applyOutcome(run.data);
          return;
        }
        // A failed scan arrives as the serialised error; keep the message.
        const store = useLibraryStore.getState();
        store.applyOutcome(null);
        void store.loadScanStatus();
      });

      if (disposed) {
        progressUnlisten();
        completeUnlisten();
        return;
      }
      unlisteners.push(progressUnlisten, completeUnlisten);
    };

    void attach();

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  }, []);
}
