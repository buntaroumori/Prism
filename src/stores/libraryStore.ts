import { create } from "zustand";

import { toUiError, type UiError } from "@/lib/errors";
import { libraryApi, systemApi } from "@/lib/ipc";
import type {
  AlbumRow,
  LibraryRoot,
  LibraryStats,
  ScanProgress,
  ScanRun,
  ScanStatus,
} from "@/types/ipc";

interface LibraryStore {
  stats: LibraryStats | null;
  /** Global search text, owned by the top bar and consumed by every view. */
  query: string;
  roots: LibraryRoot[];
  recent: AlbumRow[];
  scan: ScanStatus | null;
  progress: ScanProgress | null;
  error: UiError | null;
  loading: boolean;

  loadOverview: () => Promise<void>;
  setQuery: (query: string) => void;
  refreshStats: () => Promise<void>;
  loadRoots: () => Promise<void>;
  loadRecent: (limit?: number) => Promise<void>;
  loadScanStatus: () => Promise<void>;
  addRoot: (path: string) => Promise<boolean>;
  removeRoot: (id: string) => Promise<void>;
  setRootEnabled: (id: string, enabled: boolean) => Promise<void>;
  startScan: (options?: { full?: boolean; prune?: boolean }) => Promise<void>;
  cancelScan: () => Promise<void>;
  applyProgress: (progress: ScanProgress) => void;
  applyOutcome: (run: ScanRun | null) => void;
  clearError: () => void;
}

/**
 * Catalogue data for the shell: counters, roots, the recent-additions rail and
 * the scan state machine.
 *
 * Errors are stored rather than thrown so a failed refresh degrades to a banner
 * on top of the last good data — the same policy the diagnostics screen used in
 * FASE 1, applied consistently.
 */
export const useLibraryStore = create<LibraryStore>()((set, get) => ({
  stats: null,
  query: "",
  roots: [],
  recent: [],
  scan: null,
  progress: null,
  error: null,
  loading: false,

  loadOverview: async () => {
    set({ loading: true });
    try {
      const [stats, roots, recent, scan] = await Promise.all([
        systemApi.getLibraryStats(),
        libraryApi.roots(),
        libraryApi.albums({ sort: "added", limit: 18 }),
        libraryApi.scanStatus(),
      ]);
      set({
        stats,
        roots,
        recent: recent.items,
        scan,
        progress: scan.progress,
        error: null,
      });
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ loading: false });
    }
  },

  setQuery: (query) => {
    set({ query });
  },

  refreshStats: async () => {
    try {
      const stats = await systemApi.getLibraryStats();
      set({ stats, error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  loadRoots: async () => {
    try {
      const roots = await libraryApi.roots();
      set({ roots, error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  loadRecent: async (limit = 18) => {
    try {
      const page = await libraryApi.albums({ sort: "added", limit });
      set({ recent: page.items });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  loadScanStatus: async () => {
    try {
      const scan = await libraryApi.scanStatus();
      set({ scan, progress: scan.progress });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  addRoot: async (path) => {
    try {
      await libraryApi.addRoot(path);
      await get().loadRoots();
      set({ error: null });
      return true;
    } catch (cause) {
      set({ error: toUiError(cause) });
      return false;
    }
  },

  removeRoot: async (id) => {
    try {
      await libraryApi.removeRoot(id);
      await get().loadRoots();
      set({ error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  setRootEnabled: async (id, enabled) => {
    try {
      await libraryApi.setRootEnabled(id, enabled);
      await get().loadRoots();
      set({ error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  startScan: async (options) => {
    try {
      await libraryApi.scanStart(options);
      set({ error: null });
      await get().loadScanStatus();
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  cancelScan: async () => {
    try {
      await libraryApi.scanCancel();
      set({ error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  applyProgress: (progress) => {
    set({ progress, scan: { running: true, progress, lastRun: get().scan?.lastRun ?? null } });
  },

  applyOutcome: (run) => {
    set({
      progress: null,
      scan: { running: false, progress: null, lastRun: run },
    });
    // Counter, recent rail and root totals all change after a scan.
    void get().loadOverview();
  },

  clearError: () => {
    set({ error: null });
  },
}));
