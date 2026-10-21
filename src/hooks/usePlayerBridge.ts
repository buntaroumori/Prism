import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { hasBackend } from "@/lib/cover";
import {
  parseEngineEvent,
  parsePlayerTick,
  EVENT_CHANNELS,
  libraryApi,
  playerApi,
} from "@/lib/ipc";
import { usePlaybackStore } from "@/stores/playbackStore";

/**
 * Wire the engine to the playback store exactly once per window.
 *
 * Three sources, one owner:
 *   * `player_status` on mount — the shell can be reloaded (HMR, devtools)
 *     while a track is playing, so state is never assumed to be empty;
 *   * `player://event` — rare, meaningful transport changes;
 *   * `player://tick` — the 8 Hz playhead, which writes a two-field object into
 *     its own store slice so it cannot re-render the library.
 *
 * Every payload is zod-parsed: a backend/frontend skew drops the event and logs
 * instead of writing `undefined` into the store.
 */
export function usePlayerBridge(): void {
  useEffect(() => {
    // `vite preview` and the docs build run without the Tauri bridge; there is
    // no engine to listen to, and `listen` would throw on every mount.
    if (!hasBackend()) {
      return;
    }

    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const attach = async (): Promise<void> => {
      const store = usePlaybackStore.getState();
      await store.hydrate();

      const eventUnlisten = await listen<unknown>(EVENT_CHANNELS.playerEvent, (message) => {
        const event = parseEngineEvent(message.payload);
        if (event === null) {
          console.warn("Dropped a malformed player event", message.payload);
          return;
        }
        usePlaybackStore.getState().applyEvent(event);
        // The Home rails are stated in terms of listening ("recently played",
        // "on repeat"), and those aggregates are maintained by the catalogue,
        // not inferred in the renderer. A play is recorded when the engine
        // announces it, once per track.
        if (event.type === "trackChanged") {
          void libraryApi.notePlay(event.track.trackId).catch(() => {
            // A track that disappeared between queueing and playback has no row
            // to count against; the engine already reported the skip.
          });
        }
      });

      const tickUnlisten = await listen<unknown>(EVENT_CHANNELS.playerTick, (message) => {
        const tick = parsePlayerTick(message.payload);
        if (tick !== null) {
          usePlaybackStore.getState().applyTick(tick);
        }
      });

      if (disposed) {
        eventUnlisten();
        tickUnlisten();
        return;
      }
      unlisteners.push(eventUnlisten, tickUnlisten);
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

/**
 * Subscribe to the engine once at mount and return the current snapshot.
 *
 * Split from `usePlayerBridge` so a component can request hydration without
 * duplicating listeners when the shell remounts under StrictMode.
 */
export function useEngineSnapshot(): void {
  useEffect(() => {
    if (!hasBackend()) {
      return;
    }
    void playerApi.status().then((status) => {
      usePlaybackStore.setState({ status });
    });
  }, []);
}
