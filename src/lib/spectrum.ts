import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { hasBackend } from "@/lib/cover";
import { EVENT_CHANNELS, parseSpectrumFrame, visualiserApi } from "@/lib/ipc";
import type { SpectrumFrame } from "@/types/ipc";

/**
 * Spectrum frame bus.
 *
 * One Tauri listener for the whole window, reference-counted: the first
 * visualiser that mounts enables the audio-thread tap, the last one to unmount
 * disables it. Frames are delivered to subscribers through a callback instead of
 * a Zustand store on purpose — a 60 Hz store write would re-render the React tree
 * sixty times a second, which is precisely the cost a canvas exists to avoid.
 */

type Listener = (frame: SpectrumFrame) => void;

const listeners = new Set<Listener>();
let unlisten: UnlistenFn | null = null;
let acquiring: Promise<void> | null = null;
let refcount = 0;
let lastFrame: SpectrumFrame | null = null;

function deliver(frame: SpectrumFrame): void {
  lastFrame = frame;
  for (const listener of listeners) {
    listener(frame);
  }
}

async function attach(): Promise<void> {
  if (!hasBackend() || unlisten !== null || acquiring !== null) {
    return;
  }
  acquiring = (async () => {
    try {
      unlisten = await listen<unknown>(EVENT_CHANNELS.visualiserFrame, (message) => {
        const frame = parseSpectrumFrame(message.payload);
        if (frame !== null) {
          deliver(frame);
        }
      });
      await visualiserApi.setEnabled(true);
    } catch (cause) {
      // A build without the visualiser command, or a torn-down webview: degrade
      // to an idle canvas rather than throwing during unmount.
      console.warn("Spectrum tap unavailable", cause);
      unlisten?.();
      unlisten = null;
    } finally {
      acquiring = null;
    }
  })();
  await acquiring;
}

async function detach(): Promise<void> {
  if (unlisten === null) {
    return;
  }
  const stop = unlisten;
  unlisten = null;
  stop();
  try {
    await visualiserApi.setEnabled(false);
  } catch (cause) {
    console.warn("Cannot release the spectrum tap", cause);
  }
  lastFrame = null;
}

/**
 * Start receiving frames. Returns the matching release function.
 *
 * Safe to call in a `useEffect` with `[]` deps: releasing twice is a no-op.
 */
export function acquireSpectrum(): () => void {
  refcount += 1;
  void attach();
  let released = false;
  return () => {
    if (released) {
      return;
    }
    released = true;
    refcount = Math.max(0, refcount - 1);
    if (refcount === 0) {
      void detach();
    }
  };
}

/** Subscribe to frames. Returns the unsubscribe function. */
export function subscribeSpectrum(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Most recent frame, for a component that mounts mid-playback. */
export function latestSpectrum(): SpectrumFrame | null {
  return lastFrame;
}
