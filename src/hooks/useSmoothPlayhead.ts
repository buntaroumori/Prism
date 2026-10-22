import { useEffect, useRef, useState } from "react";

/**
 * How often the extrapolated value is published, in milliseconds.
 *
 * Thirty frames per second is the point where a progress fill and a clock read
 * as continuous, and it keeps the work to one small subtree: the value is
 * consumed by the scrubber, never by the library.
 */
const STEP_MS = 33;

/**
 * Playhead that never stands still.
 *
 * The engine publishes eight ticks per second, which is enough to keep the
 * labels honest but not enough to paint a smooth bar — and if a tick is ever
 * lost (a busy webview, a suspended window, a device change) a naive consumer
 * freezes until the next one arrives. Both failures look identical from the
 * outside: "the position does not advance".
 *
 * So the tick is treated as an *anchor*, not as the position. Between anchors
 * the wall clock does the arithmetic: the hook advances the value while the
 * transport runs, snaps to every new anchor, and stops the moment playback
 * pauses. A missing tick costs nothing, because the next frame is computed from
 * the last anchor rather than from the missing event.
 *
 * `durationMs` bounds the extrapolation so a stalled engine cannot walk the bar
 * past the end of the track.
 */
export function useSmoothPlayhead(
  positionMs: number,
  playing: boolean,
  durationMs: number | null,
): number {
  const anchor = useRef({ positionMs, at: performance.now() });
  const [position, setPosition] = useState(positionMs);

  // Every tick, and every seek (which writes one), re-anchors the clock.
  useEffect(() => {
    anchor.current = { positionMs, at: performance.now() };
    setPosition(positionMs);
  }, [positionMs]);

  useEffect(() => {
    if (!playing) {
      return;
    }
    let frame = 0;
    let published = anchor.current;
    const step = (): void => {
      const now = performance.now();
      const elapsed = now - anchor.current.at;
      const next = anchor.current.positionMs + elapsed;
      const bounded = durationMs === null ? next : Math.min(next, durationMs);
      if (now - published.at >= STEP_MS) {
        published = { positionMs: bounded, at: now };
        setPosition(Math.max(0, bounded));
      }
      frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [playing, durationMs, positionMs]);

  return Math.max(0, position);
}
