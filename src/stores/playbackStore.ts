import { create } from "zustand";

import { toUiError, type UiError } from "@/lib/errors";
import { libraryApi, playerApi } from "@/lib/ipc";
import type {
  AlbumCollection,
  AlbumRow,
  AlbumSort,
  EngineEvent,
  PlayerStatus,
  PlayerTick,
  TrackRequest,
  TrackRow,
} from "@/types/ipc";
import { usePlaybackPreferencesStore } from "./playbackPreferencesStore";

/** One entry of the playback queue. */
export interface QueueEntry {
  /** Stable identity of this occurrence, including repeated tracks. */
  queueId: string;
  /** Album occurrences share a group; individual tracks have their own group. */
  groupId: string;
  groupKind: "album" | "track";
  groupTitle: string;
  /** Explicit Play-next material is kept ahead of the remaining context. */
  manual: boolean;
  trackId: string;
  title: string;
  artist: string;
  album: string;
  /**
   * Catalogue identifiers of the record and the artist.
   *
   * The engine knows them only as text (`TrackRequest` carries the display
   * strings), but the queue was built from library rows that have the ids —
   * keeping them here is what lets the player bar link the title to its album
   * and the artist to their discography without a second round-trip.
   */
  albumId: string | null;
  artistId: string | null;
  path: string;
  durationMs: number;
  startMs?: number | null;
  endMs?: number | null;
  /** Full catalogue row retained for artwork and context-menu actions. */
  catalogue: TrackRow;
}

export interface PlayOptions {
  shuffle?: boolean;
  album?: AlbumRow | null;
  continuation?: LibraryContinuation | null;
}

export interface LibraryContinuation {
  currentAlbumId: string;
  text?: string;
  artistId?: string;
  genre?: string;
  yearFrom?: number;
  yearTo?: number;
  collection?: AlbumCollection;
  sort?: AlbumSort;
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
  deviceFormat: null,
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
  /**
   * The level the user released the slider on, held until the engine reports it
   * back.
   *
   * Without this the bar snaps to the stale snapshot the instant the drag ends:
   * `player_set_volume` returns before the next `player_status` is fetched, so
   * the control would redraw at the previous level and look like it refused the
   * gesture — the reported "I can lower it but I cannot slide it".
   */
  committedVolumeDb: number | null;
  error: UiError | null;
  /**
   * A track the engine had to skip. Recoverable by definition — playback moved
   * on — so it is a notice, not an error: it explains the gap and is dismissible.
   */
  notice: { title: string; message: string } | null;
  /** True while a transport command is in flight (guards double clicks). */
  busy: boolean;
  /** Unshuffled order; its presence is also the shared shuffle-active state. */
  shuffleSnapshot: QueueEntry[] | null;
  /** The library query that originated an album, used only when opted in. */
  continuation: LibraryContinuation | null;
  continuing: boolean;

  hydrate: () => Promise<void>;
  /** Re-read the engine snapshot; the only way `status` becomes current. */
  refresh: () => Promise<void>;
  applyEvent: (event: EngineEvent) => void;
  applyTick: (tick: PlayerTick) => void;
  play: (entries: QueueEntry[], index: number, options?: PlayOptions) => Promise<void>;
  playTracks: (tracks: TrackRow[], index: number, options?: PlayOptions) => Promise<void>;
  enqueueNext: (tracks: TrackRow[], album?: AlbumRow | null) => Promise<void>;
  updateQueue: (queue: QueueEntry[]) => Promise<boolean>;
  jumpTo: (index: number) => Promise<void>;
  removeQueueEntry: (queueId: string) => Promise<void>;
  removeQueueGroup: (groupId: string) => Promise<void>;
  moveQueueEntry: (queueId: string, beforeQueueId: string | null) => Promise<void>;
  moveQueueGroup: (groupId: string, beforeQueueId: string | null) => Promise<void>;
  toggleShuffle: () => Promise<void>;
  extendContinuation: (force?: boolean) => Promise<boolean>;
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
export function durationMs(
  status: PlayerStatus | null,
  tick: PlayerTick | null,
  catalogueMs: number | null = null,
): number | null {
  if (tick?.durationMs !== null && tick?.durationMs !== undefined && tick.durationMs > 0) {
    return tick.durationMs;
  }
  if (status?.durationFrames !== null && status?.durationFrames !== undefined) {
    const rate = status.sourceFormat?.sampleRate ?? 0;
    if (rate > 0) {
      return Math.round((status.durationFrames * 1000) / rate);
    }
  }
  // The engine only knows a length once the container declares one. The
  // catalogue indexed the same file with the same header, so its duration is a
  // truthful fallback — and a bar that is never disabled is worth more than a
  // bar that is disabled "honestly" over a stream that plays perfectly well.
  if (catalogueMs !== null && catalogueMs > 0) {
    return catalogueMs;
  }
  return null;
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

let queueSequence = 0;
function nextQueueId(): string {
  queueSequence += 1;
  return `q-${Date.now().toString(36)}-${queueSequence.toString(36)}`;
}

/** Convert a library row to a queue entry (CUE spans preserved). */
function fromRow(
  row: TrackRow,
  group: { id: string; kind: "album" | "track"; title: string; manual: boolean },
): QueueEntry {
  return {
    queueId: nextQueueId(),
    groupId: group.id,
    groupKind: group.kind,
    groupTitle: group.title,
    manual: group.manual,
    trackId: row.id,
    title: row.title,
    artist: row.artist,
    album: row.album,
    path: row.path,
    albumId: row.albumId,
    artistId: row.artistId,
    durationMs: row.durationMs,
    startMs: row.cueStartMs,
    endMs: row.cueEndMs,
    catalogue: row,
  };
}

function fromRows(
  rows: TrackRow[],
  album: AlbumRow | null | undefined,
  manual: boolean,
): QueueEntry[] {
  const groupId = `${album === null || album === undefined ? "tracks" : "album"}:${nextQueueId()}`;
  return rows.map((row) =>
    fromRow(row, {
      id: album === null || album === undefined ? `track:${nextQueueId()}` : groupId,
      kind: album === null || album === undefined ? "track" : "album",
      title: album?.title ?? row.album,
      manual,
    }),
  );
}

function randomUnit(): number {
  const value = new Uint32Array(1);
  crypto.getRandomValues(value);
  return (value[0] ?? 0) / 0x1_0000_0000;
}

/** Fisher–Yates with fresh entropy for every user action. */
function shuffled<T>(items: T[], avoidFirst?: string): T[] {
  const result = [...items];
  for (let index = result.length - 1; index > 0; index -= 1) {
    const target = Math.floor(randomUnit() * (index + 1));
    [result[index], result[target]] = [result[target] as T, result[index] as T];
  }
  if (
    avoidFirst !== undefined &&
    result.length > 1 &&
    (result[0] as T & { queueId?: string }).queueId === avoidFirst
  ) {
    const target = 1 + Math.floor(randomUnit() * (result.length - 1));
    [result[0], result[target]] = [result[target] as T, result[0] as T];
  }
  return result;
}

function insertManualEntries(
  queue: QueueEntry[],
  currentIndex: number,
  entries: QueueEntry[],
): QueueEntry[] {
  const rest = queue.slice(currentIndex + 1);
  let lastManual = -1;
  rest.forEach((entry, index) => {
    if (entry.manual) lastManual = index;
  });
  const position = currentIndex + lastManual + 2;
  return [...queue.slice(0, position), ...entries, ...queue.slice(position)];
}

async function albumTracks(album: AlbumRow): Promise<TrackRow[]> {
  const tracks: TrackRow[] = [];
  let offset = 0;
  for (;;) {
    const page = await libraryApi.search({
      albumId: album.id,
      sort: "album",
      limit: 500,
      offset,
    });
    tracks.push(...page.items);
    offset += page.items.length;
    if (page.items.length === 0 || offset >= page.total) return tracks;
  }
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
  committedVolumeDb: null,
  error: null,
  notice: null,
  busy: false,
  shuffleSnapshot: null,
  continuation: null,
  continuing: false,

  hydrate: async () => {
    try {
      const status = await playerApi.status();
      set({
        status,
        index: status.queueIndex,
        pendingVolumeDb: null,
        committedVolumeDb: null,
        error: null,
      });
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  refresh: async () => {
    try {
      const status = await playerApi.status();
      set((state) => ({
        status,
        index: status.queueIndex,
        // Once the engine reports the level the user committed, the optimistic
        // copy has done its job and must not outrank the driver any longer.
        committedVolumeDb:
          state.committedVolumeDb !== null &&
          Math.abs(status.volumeDb - state.committedVolumeDb) < 0.05
            ? null
            : state.committedVolumeDb,
        error: null,
      }));
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  applyEvent: (event) => {
    const base: PlayerStatus = get().status ?? EMPTY_STATUS;

    switch (event.type) {
      case "stateChanged": {
        if (event.state === "stopped") {
          // `Stop` tears the whole session down in the engine: queue, device and
          // current track all go. Mirroring only the state left the bar on
          // screen with a dead play button, which is why stop felt unreliable.
          set({
            status: { ...EMPTY_STATUS, volumeDb: base.volumeDb, muted: base.muted },
            tick: null,
            index: 0,
            queue: [],
            shuffleSnapshot: null,
            continuation: null,
          });
          break;
        }
        set({
          status: { ...base, state: event.state },
          // A state change invalidates the last tick: showing the old playhead
          // after a pause would leave a stale progress bar on screen.
          tick: get().tick,
        });
        break;
      }
      case "trackChanged": {
        set({
          status: { ...base, track: event.track, queueIndex: event.index, state: "buffering" },
          index: event.index,
          tick: null,
        });
        // The event carries the identity of the new track, not the snapshot:
        // its length and source rate arrive with the next `player_status`.
        void get().refresh();
        void get().extendContinuation();
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
        void get().refresh();
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

  play: async (entries, index, options = {}) => {
    if (entries.length === 0) return;
    const previous = get();
    const safeIndex = Math.min(Math.max(index, 0), entries.length - 1);
    const currentPath = previous.status?.track?.path;
    const avoidFirst =
      currentPath === undefined
        ? undefined
        : entries.slice(safeIndex).find((entry) => entry.path === currentPath)?.queueId;
    const snapshot = options.shuffle === true ? [...entries] : null;
    const queue =
      options.shuffle === true
        ? [...entries.slice(0, safeIndex), ...shuffled(entries.slice(safeIndex), avoidFirst)]
        : entries;
    set({
      busy: true,
      queue,
      index: safeIndex,
      tick: null,
      shuffleSnapshot: snapshot,
      continuation: options.continuation ?? null,
    });
    try {
      await playerApi.play(queue.map(toRequest), safeIndex);
      set({ error: null });
    } catch (cause) {
      set({
        queue: previous.queue,
        index: previous.index,
        tick: previous.tick,
        shuffleSnapshot: previous.shuffleSnapshot,
        continuation: previous.continuation,
        error: toUiError(cause),
        busy: false,
      });
      return;
    }
    set({ busy: false });
    void get().refresh();
    void get().extendContinuation();
  },

  playTracks: async (tracks, index, options = {}) => {
    await get().play(fromRows(tracks, options.album, false), index, options);
  },

  enqueueNext: async (tracks, album = null) => {
    const entries = fromRows(tracks, album, true);
    if (entries.length === 0) return;
    const state = get();
    if (
      state.status?.track === null ||
      state.status?.track === undefined ||
      state.queue.length === 0
    ) {
      await get().play(entries, 0);
      return;
    }
    const queue = insertManualEntries(state.queue, state.index, entries);
    const previousSnapshot = state.shuffleSnapshot;
    if (previousSnapshot !== null) {
      const currentId = state.queue[state.index]?.queueId;
      const snapshotIndex = previousSnapshot.findIndex((entry) => entry.queueId === currentId);
      if (snapshotIndex >= 0) {
        set({
          shuffleSnapshot: insertManualEntries(previousSnapshot, snapshotIndex, entries),
        });
      }
    }
    if (!(await get().updateQueue(queue))) {
      set({ shuffleSnapshot: previousSnapshot });
    }
  },

  updateQueue: async (queue) => {
    const state = get();
    const currentId = state.queue[state.index]?.queueId;
    const index = queue.findIndex((entry) => entry.queueId === currentId);
    if (index < 0) {
      set({ error: { code: "queue", message: "The current song is no longer in the queue." } });
      return false;
    }
    set({ queue, index });
    try {
      await playerApi.updateQueue(queue.map(toRequest), index);
      set({ error: null });
      return true;
    } catch (cause) {
      set({ queue: state.queue, index: state.index, error: toUiError(cause) });
      return false;
    }
  },

  jumpTo: async (index) => {
    const state = get();
    if (state.queue[index] === undefined) return;
    set({ busy: true });
    try {
      await playerApi.play(state.queue.map(toRequest), index);
      set({ index, tick: null, error: null });
      void get().refresh();
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  removeQueueEntry: async (queueId) => {
    const state = get();
    if (state.queue[state.index]?.queueId === queueId) return;
    const previousSnapshot = state.shuffleSnapshot;
    set({
      shuffleSnapshot: previousSnapshot?.filter((entry) => entry.queueId !== queueId) ?? null,
    });
    const queue = state.queue.filter((entry) => entry.queueId !== queueId);
    if (!(await get().updateQueue(queue))) set({ shuffleSnapshot: previousSnapshot });
  },

  removeQueueGroup: async (groupId) => {
    const state = get();
    const currentId = state.queue[state.index]?.queueId;
    const previousSnapshot = state.shuffleSnapshot;
    set({
      shuffleSnapshot:
        previousSnapshot?.filter(
          (entry) => entry.groupId !== groupId || entry.queueId === currentId,
        ) ?? null,
    });
    // Removing the current album clears its remaining tracks but keeps the
    // loaded song alive until the listener explicitly skips or stops it.
    const queue = state.queue.filter(
      (entry) => entry.groupId !== groupId || entry.queueId === currentId,
    );
    if (!(await get().updateQueue(queue))) set({ shuffleSnapshot: previousSnapshot });
  },

  moveQueueEntry: async (queueId, beforeQueueId) => {
    const state = get();
    const sourceIndex = state.queue.findIndex((entry) => entry.queueId === queueId);
    if (sourceIndex <= state.index) return;
    const source = state.queue[sourceIndex];
    if (source === undefined) return;
    const remaining = state.queue.filter((entry) => entry.queueId !== queueId);
    const targetIndex =
      beforeQueueId === null
        ? remaining.length
        : remaining.findIndex((entry) => entry.queueId === beforeQueueId);
    if (beforeQueueId !== null && targetIndex <= state.index) return;
    const queue = [...remaining];
    queue.splice(targetIndex < 0 ? queue.length : targetIndex, 0, source);
    await get().updateQueue(queue);
  },

  moveQueueGroup: async (groupId, beforeQueueId) => {
    const state = get();
    if (state.queue[state.index]?.groupId === groupId) return;
    const moving = state.queue.filter(
      (entry, index) => entry.groupId === groupId && index > state.index,
    );
    if (moving.length === 0) return;
    const ids = new Set(moving.map((entry) => entry.queueId));
    const remaining = state.queue.filter((entry) => !ids.has(entry.queueId));
    const targetIndex =
      beforeQueueId === null
        ? remaining.length
        : remaining.findIndex((entry) => entry.queueId === beforeQueueId);
    if (beforeQueueId !== null && targetIndex <= state.index) return;
    const queue = [...remaining];
    queue.splice(targetIndex < 0 ? queue.length : targetIndex, 0, ...moving);
    await get().updateQueue(queue);
  },

  toggleShuffle: async () => {
    const state = get();
    if (state.queue.length === 0) return;
    if (state.shuffleSnapshot !== null) {
      const snapshot = state.shuffleSnapshot;
      set({ shuffleSnapshot: null });
      if (!(await get().updateQueue(snapshot))) set({ shuffleSnapshot: snapshot });
      return;
    }
    const snapshot = [...state.queue];
    const queue = [
      ...state.queue.slice(0, state.index + 1),
      ...shuffled(state.queue.slice(state.index + 1)),
    ];
    set({ shuffleSnapshot: snapshot });
    if (!(await get().updateQueue(queue))) set({ shuffleSnapshot: null });
  },

  extendContinuation: async (force = false) => {
    const state = get();
    const continuation = state.continuation;
    if (
      continuation === null ||
      state.continuing ||
      state.index !== state.queue.length - 1 ||
      (!force && usePlaybackPreferencesStore.getState().albumEnd !== "continue-library")
    ) {
      return false;
    }
    set({ continuing: true });
    try {
      const query = {
        ...(continuation.text !== undefined ? { text: continuation.text } : {}),
        ...(continuation.artistId !== undefined ? { artistId: continuation.artistId } : {}),
        ...(continuation.genre !== undefined ? { genre: continuation.genre } : {}),
        ...(continuation.yearFrom !== undefined ? { yearFrom: continuation.yearFrom } : {}),
        ...(continuation.yearTo !== undefined ? { yearTo: continuation.yearTo } : {}),
        ...(continuation.collection !== undefined ? { collection: continuation.collection } : {}),
        sort: continuation.sort ?? ("artist" as const),
      };
      let offset = 0;
      let nextAlbum: AlbumRow | undefined;
      for (;;) {
        const page = await libraryApi.albums({
          ...query,
          limit: 500,
          offset,
        });
        const current = page.items.findIndex((album) => album.id === continuation.currentAlbumId);
        if (current >= 0) {
          nextAlbum = page.items[current + 1];
          if (nextAlbum === undefined && offset + page.items.length < page.total) {
            const following = await libraryApi.albums({
              ...query,
              limit: 1,
              offset: offset + page.items.length,
            });
            nextAlbum = following.items[0];
          }
          break;
        }
        offset += page.items.length;
        if (page.items.length === 0 || offset >= page.total) break;
      }
      if (nextAlbum === undefined) return false;
      const entries = fromRows(await albumTracks(nextAlbum), nextAlbum, false);
      if (entries.length === 0) return false;
      const latest = get();
      if (latest.continuation?.currentAlbumId !== continuation.currentAlbumId) {
        return false;
      }
      const queue = [...latest.queue, ...entries];
      await playerApi.updateQueue(queue.map(toRequest), latest.index);
      set((current) => ({
        queue,
        continuation: { ...continuation, currentAlbumId: nextAlbum.id },
        shuffleSnapshot:
          current.shuffleSnapshot === null ? null : [...current.shuffleSnapshot, ...entries],
        error: null,
      }));
      return true;
    } catch (cause) {
      set({ error: toUiError(cause) });
      return false;
    } finally {
      set({ continuing: false });
    }
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
      void get().refresh();
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  next: async () => {
    const { index } = get();
    if (index + 1 < get().queue.length) {
      await get().jumpTo(index + 1);
    } else if (await get().extendContinuation()) {
      await get().jumpTo(get().index + 1);
    }
  },

  previous: async () => {
    const { index, status } = get();
    // Standard transport behaviour: restart the track unless we are near the
    // start, in which case step back in the queue.
    const position = playheadMs(status, get().tick);
    const target = position > 3_000 ? index : Math.max(index - 1, 0);
    if (position > 3_000) {
      await get().seekMs(0);
      return;
    }
    await get().jumpTo(target);
  },

  stop: async () => {
    set({ busy: true });
    try {
      await playerApi.stop();
      // Optimistic teardown: the engine emits `stateChanged(stopped)` too, but
      // the bar must disappear on the click, not a round trip later.
      set({
        tick: null,
        index: 0,
        queue: [],
        shuffleSnapshot: null,
        continuation: null,
        status:
          get().status === null
            ? null
            : {
                ...EMPTY_STATUS,
                volumeDb: get().status?.volumeDb ?? 0,
                muted: get().status?.muted ?? false,
              },
        error: null,
      });
    } catch (cause) {
      set({ error: toUiError(cause) });
    } finally {
      set({ busy: false });
    }
  },

  seekMs: async (ms) => {
    const status = get().status;
    const target = Math.max(0, Math.round(ms));
    // Optimistic: the slider follows the click immediately; the next tick
    // corrects it once the device has actually flushed.
    set({
      tick: {
        positionMs: target,
        durationMs: get().tick?.durationMs ?? null,
        playing: get().tick?.playing ?? status?.state === "playing",
      },
    });
    try {
      await playerApi.seek(target);
      void get().refresh();
    } catch (cause) {
      set({ error: toUiError(cause) });
    }
  },

  setVolumeDb: async (db) => {
    set({ committedVolumeDb: db });
    try {
      await playerApi.setVolumeDb(db);
      await get().refresh();
      set({ pendingVolumeDb: null, error: null });
    } catch (cause) {
      set({ error: toUiError(cause), pendingVolumeDb: null, committedVolumeDb: null });
    }
  },

  previewVolumeDb: (db) => {
    set({ pendingVolumeDb: db });
    // The engine owns the gain ramp, so every pointer update is audible while
    // the gesture is still in progress rather than only on pointer release.
    void playerApi.setVolumeDb(db).catch((cause: unknown) => {
      set({ error: toUiError(cause) });
    });
  },

  setMuted: async (muted) => {
    const previous = get().status?.muted ?? false;
    // Mute has no transport event of its own. Update the control immediately
    // and then read the engine back, instead of waiting for an unrelated play
    // or seek to refresh the snapshot.
    set((state) => ({
      status: state.status === null ? state.status : { ...state.status, muted },
    }));
    try {
      await playerApi.setMuted(muted);
      await get().refresh();
      set({ error: null });
    } catch (cause) {
      set((state) => ({
        status: state.status === null ? state.status : { ...state.status, muted: previous },
        error: toUiError(cause),
      }));
    }
  },

  clearError: () => {
    set({ error: null });
  },

  clearNotice: () => {
    set({ notice: null });
  },
}));
