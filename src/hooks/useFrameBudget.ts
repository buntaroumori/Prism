import { useEffect } from "react";

import { useUiStore } from "@/stores/uiStore";

/** Frames sampled before deciding. */
const SAMPLES = 45;
/** Wait after the first paint so bundle evaluation is not measured as jank. */
const SETTLE_MS = 1_500;
/** Median frame budget above which the expensive layers are dropped. */
const SLOW_FRAME_MS = 24;
/** One decision per session: a resize or a busy moment must not flap it. */
let measured = false;

/**
 * Decide, from the machine's own behaviour, how expensive the interface may be.
 *
 * The same build is fluid on one desktop and visibly torpid on another with
 * identical hardware — the difference is the compositor. A Linux partition
 * without a working GL driver composites WebKit in software, and every pixel of
 * `backdrop-filter` and every 54-rem blurred orb is then paid for on the CPU, at
 * the expense of the audio thread and of every scroll.
 *
 * Rather than guess from the platform name (wrong at least half the time), the
 * shell measures its own frame pacing once the page has settled and, if the
 * median frame takes longer than a 42 Hz budget, asks the store to shed the
 * expensive layers. The preference stays overridable: `auto` is the default,
 * `full` and `reduced` are explicit choices.
 */
export function useFrameBudget(): void {
  useEffect(() => {
    if (measured) {
      return;
    }
    let cancelled = false;
    let timer = 0;
    let frame = 0;

    const measure = (): void => {
      const intervals: number[] = [];
      let previous = performance.now();
      const tick = (): void => {
        const now = performance.now();
        intervals.push(now - previous);
        previous = now;
        if (intervals.length < SAMPLES) {
          frame = requestAnimationFrame(tick);
          return;
        }
        measured = true;
        if (cancelled) {
          return;
        }
        intervals.sort((left, right) => left - right);
        const median = intervals[Math.floor(intervals.length / 2)] ?? SLOW_FRAME_MS;
        useUiStore.getState().setMeasuredEffects(median > SLOW_FRAME_MS ? "reduced" : "full");
      };
      frame = requestAnimationFrame(tick);
    };

    // Give the first paint, the fonts and the initial library query time to
    // land; measuring during boot would condemn every machine.
    timer = window.setTimeout(measure, SETTLE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      window.cancelAnimationFrame(frame);
    };
  }, []);
}
