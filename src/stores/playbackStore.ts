import { create } from "zustand";

import { toUiError, type UiError } from "@/lib/errors";
import { playerApi } from "@/lib/ipc";
import type { EngineEvent, PlayerStatus, PlayerTick, TrackRequest, TrackRow } from "@/types/ipc";

/** One entry of the playback queue. */
export interface QueueEntry {
  trackId: string;
  title: string;
  artist: string;
  album: string;
  path: string;
  durationMs: number;
  startMs?: number | null;
  endMs?: number | null;
}

/**
 * Neutral engine snapshot.
 *
 * Used before the first `player_status` answer, and by the player bar when the
 * transport is visible but the store has not caught up yet. Exported so there is
 * exactly one definition of "nothing is playing" in the frontend.
 */
export const EMPTY_STATUS: PlayerStatus = {
  state: "stopped",
  track: null,
  positionFrames: 0,
  durationFrames: null,
  sourceFormat: null,
  outputFormat: null,
  backend: null,
  deviceName: null,
  bitPerfect: false,
  dop: false,
  volumeDb: 0,
  muted: false,
  latencyFrames: 0,
  underruns: 0,
  overruns: 0,
  queueIndex: 0,
  queueLen: 0,
};

interface PlaybackStore {
  /** Latest full engine snapshot; `null` until the first `player_status`. */
  status: PlayerStatus | null;
  /** Latest playhead projection from `player://tick`. */
  tick: PlayerTick | null;
  queue: QueueEntry[];
  index: number;
  /** Optimistic volume while the user drags; the engine echoes the truth. */
  pendingVolumeDb: number | null;
  error: UiError | null;
  /**
   * A track the engine had to skip. Recoverable by definition — playback moved
   * on — so it is a notice, not an error: it explains the gap and is dismissible.
   */
  notice: { title: string; message: string } | null;
  /** True while a transport command is in flight (guards double clicks). */
  busy: boolean;

  hydrate: () => Promise<void>;
  applyEvent: (event: EngineEvent) => void;
  applyTick: (tick: PlayerTick) => void;
  play: (entries: QueueEntry[], index: number) => Promise<void>;
  playTracks: (tracks: TrackRow[], index: number) => Promise<void>;
  toggle: () => Promise<void>;
  next: () => Promise<void>;
  previous: () => Promise<void>;
  stop: () => Promise<void>;
  seekMs: (ms: number) => Promise<void>;
  setVolumeDb: (db: number) => Promise<void>;
  previewVolumeDb: (db: number) => void;
  setMuted: (muted: boolean) => Promise<void>;
  clearError: () => void;
  clearNotice: () => void;
}

/**
 * Position of the playhead in milliseconds.
 *
 * The tick is authoritative while the transport runs; the engine snapshot is
 * used for the first frame after a seek so the slider never snaps back to the
 * previous position.
 */
export function playheadMs(status: PlayerStatus | null, tick: PlayerTick | null): number {
  if (tick !== null) {
    return tick.positionMs;
  }
  if (status === null) {
    return 0;
  }
  const rate = status.sourceFormat?.sampleRate ?? 0;
  return rate === 0 ? 0 : Math.round((status.positionFrames * 1000) / rate);
}

/** Duration of the loaded track in milliseconds, from either source. */
export function durationMs(status: PlayerStatus | null, tick: PlayerTick | null): number | null {
  if (tick?.durationMs !== null && tick?.durationMs !== undefined && tick.durationMs > 0) {
    return tick.durationMs;
  }
  if (status?.durationFrames !== null && status?.durationFrames !== undefined) {
    const rate = status.sourceFormat?.sampleRate ?? 0;
    if (rate > 0) {
      return Math.round((status.durationFrames * 1000) / rate);
    }
  }
  return status?.track !== null && status?.track !== undefined ? null : null;
}

/** Convert a queue entry to the engine contract. */
function toRequest(entry: QueueEntry): TrackRequest {
  return {
    trackId: entry.trackId,
    title: entry.title,
    artist: entry.artist,
    album: entry.album,
    path: entry.path,
    startMs: entry.startMs ?? null,
    endMs: entry.endMs ?? null,
    replaygainTrackGainDb: null,
    replaygainTrackPeak: null,
    replaygainAlbumGainDb: null,
    replaygainAlbumPeak: null,
  };
}

/** Convert a library row to a queue entry (CUE spans preserved). */
function fromRow(row: TrackRow): QueueEntry {
  return {
    trackId: row.id,
    title: row.title,
    artist: row.artist,
    album: row.album,
    path: row.path,
    durationMs: row.durationMs,
    startMs: row.cueStartMs,
    endMs: row.cueEndMs,
  };
}

/**
 * Transport state.
 *
 * Deliberately isolated from the library and shell stores: the 8 Hz playhead
 * tick must not be able to invalidate a single library row, and a sidebar
 * collapse must not repaint the player bar. Components subscribe with atomic
 * selectors (`usePlaybackStore((s) => s.status?.state)`).
 */
export const usePlaybackStore = create<PlaybackStore>()((set, get) => ({
  status: null,
  tick: null,
  queue: [],
  index: 0,
  pendingVolumeDb: null,
  error: null,
  notice: null,
  busy: false,

  hydrate: async () => {
    try {
      const status = await playerApi.status();
      set({
        status,
        index: status.queueIndex,
        pendingVolumeDb: null,
        error: null,
      });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  applyEvent: (event) => {
    const base: PlayerStatus = get().status ?? EMPTY_STATUS;

    switch (event.type) {
      case "stateChanged": {
        set({
          status: { ...base, state: event.state },
          // A state change invalidates the last tick: showing the old playhead
          // after a stop would leave a frozen progress bar on screen.
          tick: event.state === "stopped" ? null : get().tick,
        });
        break;
      }
      case "trackChanged": {
        set({
          status: { ...base, track: event.track, queueIndex: event.index, state: "buffering" },
          index: event.index,
          tick: null,
        });
        break;
      }
      case "deviceChanged": {
        set({
          status: {
            ...base,
            deviceName: event.device,
            backend: event.backend,
            outputFormat: event.format,
            bitPerfect: event.bit_perfect,
          },
        });
        break;
      }
      case "outputError": {
        set({ error: { code: "output", message: event.message } });
        break;
      }
      case "underrun": {
        set({ status: { ...base, underruns: event.total } });
        break;
      }
      case "trackSkipped": {
        set({
          notice: {
            title: event.title.length > 0 ? event.title : event.trackId,
            message: event.reason,
          },
        });
        break;
      }
      default: {
        // Exhaustive: adding a variant to the Rust enum must break this build.
        event satisfies never;
      }
    }
  },

  applyTick: (tick) => {
    set({ tick });
  },

  play: async (entries, index) => {
    if (entries.length === 0) {
      return;
    }
    const safeIndex = Math.min(Math.max(index, 0), entries.length - 1);
    set({ busy: true, queue: entries, index: safeIndex, tick: null });
    try {
      await playerApi.play(entries.map(toRequest), safeIndex);
      set({ error: null });
    } catch (cause) {
      set({ error: toUiError(cause), busy: false });
      return;
    }
    set({ busy: false });
  },

  playTracks: async (tracks, index) => {
    await get().play(tracks.map(fromRow), index);
  },

  toggle: async () => {
    const status = get().status;
    if (status === null) {
      return;
    }
    set({ busy: true });
    try {
      if (status.state === "playing" || status.state === "buffering") {
        await playerApi.pause();
      } else if (status.state === "paused") {
        await playerApi.resume();
      }
      set({ error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  next: async () => {
    const { queue, index } = get();
    if (index + 1 >= queue.length) {
      return;
    }
    const nextIndex = index + 1;
    set({ busy: true });
    try {
      await playerApi.play(queue.map(toRequest), nextIndex);
      set({ index: nextIndex, tick: null, error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  previous: async () => {
    const { queue, index, status } = get();
    // Standard transport behaviour: restart the track unless we are near the
    // start, in which case step back in the queue.
    const position = playheadMs(status, get().tick);
    const target = position > 3_000 ? index : Math.max(index - 1, 0);
    if (position > 3_000) {
      await get().seekMs(0);
      return;
    }
    set({ busy: true });
    try {
      await playerApi.play(queue.map(toRequest), target);
      set({ index: target, tick: null, error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  stop: async () => {
    set({ busy: true });
    try {
      await playerApi.stop();
      set({ tick: null, error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  seekMs: async (ms) => {
    const status = get().status;
    const rate = status?.sourceFormat?.sampleRate ?? 44_100;
    const frame = Math.max(0, Math.round((ms * rate) / 1000));
    // Optimistic: the slider follows the click immediately; the next tick
    // corrects it once the device has actually flushed.
    set({
      tick: {
        positionMs: Math.max(0, Math.round(ms)),
        durationMs: get().tick?.durationMs ?? null,
        playing: get().tick?.playing ?? status?.state === "playing",
      },
    });
    try {
      await playerApi.seek(frame);
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  setVolumeDb: async (db) => {
    try {
      await playerApi.setVolumeDb(db);
      set({ pendingVolumeDb: null, error: null });
    } catch (cause) {
      set({ error: toUiError(cause), pendingVolumeDb: null });
    }
  },

  previewVolumeDb: (db) => {
    set({ pendingVolumeDb: db });
  },

  setMuted: async (muted) => {
    try {
      await playerApi.setMuted(muted);
      set({ error: null });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  clearError: () => {
    set({ error: null });
  },

  clearNotice: () => {
    set({ notice: null });
  },
}));
