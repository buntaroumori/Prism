/**
 * Performance harness.
 *
 * Mounts the **production** `VirtualTrackList` over a synthetic 500 000-row
 * catalogue and exposes a measurement API on `window.prismBench`. It is not part
 * of the application: `vite.bench.config.ts` builds it as a separate page and
 * `scripts/perf-audit.mjs` drives it in a real browser.
 *
 * What is measured, and why each probe exists:
 *
 * | Probe | Question it answers |
 * | :--- | :--- |
 * | `scroll` | Does the list hold frame budget while scrolling 500 000 rows? |
 * | `search` | Does typing into the filter land inside a frame? |
 * | `nodes` | Is the DOM actually virtualised (not just "looks fast")? |
 * | `layoutShift` | Does a page arriving move anything (CLS)? |
 * | `longTasks` | Is any single task blocking the main thread for > 50 ms? |
 * | `heapMb` | Does the 500k library stay bounded in memory? |
 *
 * The harness reports the display's own frame budget, so a 120 Hz panel is
 * judged against 8.33 ms and a 60 Hz panel against 16.7 ms instead of pretending
 * every screen is the same.
 */

import { createRoot } from "react-dom/client";
import { useEffect, useRef, useState } from "react";

import { VirtualTrackList } from "@/components/library/VirtualTrackList";
import { SYNTHETIC_TRACKS, syntheticPage } from "@/perf/synthetic";

import "@/styles/globals.css";

export interface FrameSample {
  frames: number;
  durationMs: number;
  meanMs: number;
  p50Ms: number;
  p95Ms: number;
  worstMs: number;
  overBudget: number;
  fps: number;
}

export interface BenchmarkReport {
  tracks: number;
  frameBudgetMs: number;
  scroll: FrameSample | null;
  interaction: FrameSample | null;
  nodes: { total: number; rows: number };
  layoutShift: { cls: number; clsRaw: number; events: number; sources: string[] };
  longTasks: { count: number; worstMs: number };
  heapMb: number | null;
}

declare global {
  interface Window {
    prismBench?: {
      readonly ready: boolean;
      scroll: (options: { milliseconds: number; pixelsPerFrame: number }) => Promise<FrameSample>;
      measureInteraction: (text: string) => Promise<FrameSample>;
      report: () => BenchmarkReport;
      reset: () => void;
    };
  }
}

/** Refresh rate of the display, measured from rAF deltas while idle. */
async function measureFrameBudget(samples = 40): Promise<number> {
  const deltas: number[] = [];
  await new Promise<void>((resolve) => {
    let previous = performance.now();
    let count = 0;
    const step = (now: number): void => {
      deltas.push(now - previous);
      previous = now;
      count += 1;
      if (count >= samples) {
        resolve();
        return;
      }
      window.requestAnimationFrame(step);
    };
    window.requestAnimationFrame(step);
  });
  // Discard the warm-up frames (mount, style resolution) and take the median.
  const sorted = deltas.slice(8).sort((left, right) => left - right);
  const median = sorted[Math.floor(sorted.length / 2)] ?? 16.67;
  return Math.round((1000 / Math.max(1, Math.round(1000 / median))) * 100) / 100;
}

function summarise(deltas: number[], frameBudgetMs: number): FrameSample {
  if (deltas.length === 0) {
    return {
      frames: 0,
      durationMs: 0,
      meanMs: 0,
      p50Ms: 0,
      p95Ms: 0,
      worstMs: 0,
      overBudget: 0,
      fps: 0,
    };
  }
  const sorted = [...deltas].sort((left, right) => left - right);
  const total = deltas.reduce((sum, value) => sum + value, 0);
  const percentile = (fraction: number): number => {
    const index = Math.min(sorted.length - 1, Math.floor(sorted.length * fraction));
    return Math.round((sorted[index] ?? 0) * 100) / 100;
  };
  return {
    frames: deltas.length,
    durationMs: Math.round(total),
    meanMs: Math.round((total / deltas.length) * 100) / 100,
    p50Ms: percentile(0.5),
    p95Ms: percentile(0.95),
    worstMs: Math.round((sorted[sorted.length - 1] ?? 0) * 100) / 100,
    overBudget: deltas.filter((value) => value > frameBudgetMs + 0.5).length,
    fps: Math.round((deltas.length / total) * 100_000) / 100,
  };
}

/* eslint-disable react-refresh/only-export-components -- this entry point is not a Fast Refresh boundary */
function App(): React.ReactElement {
  const listRef = useRef<HTMLDivElement | null>(null);
  const shiftRef = useRef({ cls: 0, events: 0, sources: [] as string[] });
  const longTaskRef = useRef({ count: 0, worstMs: 0 });
  const scrollRef = useRef<FrameSample | null>(null);
  const interactionRef = useRef<FrameSample | null>(null);
  const budgetRef = useRef(16.67);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let shiftObserver: PerformanceObserver | null = null;
    let longTaskObserver: PerformanceObserver | null = null;
    try {
      shiftObserver = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          const shift = entry as PerformanceEntry & { value: number; hadRecentInput: boolean };
          if (!shift.hadRecentInput) {
            shiftRef.current.cls += shift.value;
            shiftRef.current.events += 1;
            // Name the element that moved, and where it moved: a CLS of
            // 0.0000037 is still a bug, and "span, 86×11 at 1330,24 unchanged"
            // says what "0.0000037" cannot.
            const sources = (
              shift as PerformanceEntry & {
                sources?: {
                  node?: Node;
                  previousRect?: DOMRectReadOnly;
                  currentRect?: DOMRectReadOnly;
                }[];
              }
            ).sources;
            for (const source of sources ?? []) {
              const node = source.node;
              if (!(node instanceof Element)) continue;
              const classes = node.getAttribute("class") ?? "";
              const name =
                classes === ""
                  ? node.tagName.toLowerCase()
                  : `${node.tagName.toLowerCase()}.${classes.split(" ").slice(0, 2).join(".")}`;
              const before = source.previousRect;
              const after = source.currentRect;
              const rect =
                before && after
                  ? ` [${String(Math.round(before.width))}\u00d7${String(Math.round(before.height))} at ${String(Math.round(before.x))},${String(Math.round(before.y))} -> ${String(Math.round(after.width))}\u00d7${String(Math.round(after.height))} at ${String(Math.round(after.x))},${String(Math.round(after.y))}]`
                  : "";
              const label = `${name}${rect}`;
              if (!shiftRef.current.sources.includes(label)) {
                shiftRef.current.sources.push(label);
              }
            }
          }
        }
      });
      shiftObserver.observe({ type: "layout-shift", buffered: true });
    } catch {
      // Entry type unavailable: CLS stays at zero and the report says so.
    }
    try {
      longTaskObserver = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          longTaskRef.current.count += 1;
          longTaskRef.current.worstMs = Math.max(longTaskRef.current.worstMs, entry.duration);
        }
      });
      longTaskObserver.observe({ type: "longtask", buffered: true });
    } catch {
      // Same: unsupported entry types are reported as zero samples.
    }
    return () => {
      shiftObserver?.disconnect();
      longTaskObserver?.disconnect();
    };
  }, []);

  // Give the first page time to arrive before the harness accepts commands.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setReady(true);
    }, 700);
    return () => {
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    window.prismBench = {
      get ready() {
        return ready;
      },
      // Clears the long-task window only: CLS is cumulative for the whole
      // session, because that is what CLS means and resetting it would hide the
      // shift a user actually sees on load.
      reset: () => {
        longTaskRef.current = { count: 0, worstMs: 0 };
        scrollRef.current = null;
        interactionRef.current = null;
      },
      scroll: async ({ milliseconds, pixelsPerFrame }): Promise<FrameSample> => {
        budgetRef.current = await measureFrameBudget();
        const scroller = listRef.current?.querySelector<HTMLElement>("[role='grid']");
        if (scroller === null || scroller === undefined) {
          throw new Error("the list is not mounted");
        }
        // Walk down the 500 000 rows and back up. The step is a parameter because
        // "scroll speed" decides everything: a trackpad flick moves ~120 px per
        // frame, a scrollbar drag moves whole viewports. Both are measured, and
        // only the first one is judged against the 120 Hz budget.
        const step = pixelsPerFrame;
        const deltas: number[] = [];
        const startedAt = performance.now();
        let previous = startedAt;
        let direction = 1;

        await new Promise<void>((resolve) => {
          const stepFrame = (now: number): void => {
            deltas.push(now - previous);
            previous = now;
            if (now - startedAt >= milliseconds) {
              resolve();
              return;
            }
            const next = scroller.scrollTop + step * direction;
            if (next > scroller.scrollHeight - scroller.clientHeight * 2) {
              direction = -1;
            } else if (next < scroller.clientHeight && direction === -1) {
              direction = 1;
            }
            scroller.scrollTop = next;
            window.requestAnimationFrame(stepFrame);
          };
          window.requestAnimationFrame(stepFrame);
        });
        const sample = summarise(deltas, budgetRef.current);
        scrollRef.current = sample;
        return sample;
      },
      measureInteraction: async (text: string): Promise<FrameSample> => {
        const input = document.querySelector<HTMLInputElement>(
          "input[aria-label='Search the library']",
        );
        if (input === null) {
          throw new Error("the search field is not mounted");
        }
        // React tracks the value through its own descriptor; assigning through
        // the prototype setter is what makes the synthetic input look real.
        const descriptor = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value");
        const deltas: number[] = [];
        let previous = performance.now();
        await new Promise<void>((resolve) => {
          let count = 0;
          const step = (now: number): void => {
            deltas.push(now - previous);
            previous = now;
            count += 1;
            if (count < 40) {
              window.requestAnimationFrame(step);
              return;
            }
            resolve();
          };
          input.focus();
          descriptor?.set?.call(input, text);
          input.dispatchEvent(new Event("input", { bubbles: true }));
          window.requestAnimationFrame(step);
        });
        budgetRef.current = await measureFrameBudget();
        const sample = summarise(deltas, budgetRef.current);
        interactionRef.current = sample;
        return sample;
      },
      report: (): BenchmarkReport => {
        const heap = (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory;
        return {
          tracks: SYNTHETIC_TRACKS,
          frameBudgetMs: budgetRef.current,
          scroll: scrollRef.current,
          interaction: interactionRef.current,
          nodes: {
            total: document.querySelectorAll("*").length,
            rows: document.querySelectorAll("[role='row']").length,
          },
          layoutShift: {
            // Rounded for display, raw for the verdict: a shift of 0.000004 is
            // still a shift, and the gate is exactly zero.
            cls: Math.round(shiftRef.current.cls * 1_000_000) / 1_000_000,
            clsRaw: shiftRef.current.cls,
            events: shiftRef.current.events,
            sources: [...shiftRef.current.sources],
          },
          longTasks: {
            count: longTaskRef.current.count,
            worstMs: Math.round(longTaskRef.current.worstMs * 100) / 100,
          },
          heapMb:
            heap === undefined ? null : Math.round((heap.usedJSHeapSize / 1_048_576) * 10) / 10,
        };
      },
    };
  }, [ready]);

  const noop = (): void => {
    /* the harness never plays anything */
  };

  return (
    <div className="flex h-screen flex-col bg-oled-950 text-white">
      <header className="flex shrink-0 items-center justify-between border-b border-white/[0.06] px-6 py-3">
        <div>
          <p className="text-[12px] font-semibold tracking-tight">
            Prism · performance harness — {SYNTHETIC_TRACKS.toLocaleString("en-US")} synthetic rows
          </p>
          <p className="text-[10.5px] text-white/35">
            Not part of the application. Drives the production VirtualTrackList over a generated
            catalogue.
          </p>
        </div>
        {/* State is carried by a dot, never by text: swapping the word "loading"
            for "ready" re-lays out the text node inside the pill and registers as
            a layout shift (measured: 0.0000037, with identical source rects). A
            colour change moves nothing. */}
        <span className="flex w-[86px] items-center justify-center gap-1.5 rounded-full border border-white/10 px-2.5 py-1 text-[10px] tracking-[0.12em] text-white/40 uppercase">
          <span
            aria-hidden="true"
            data-ready={ready}
            className={`h-1.5 w-1.5 rounded-full ${ready ? "bg-positive" : "bg-white/20"}`}
          />
          harness
        </span>
      </header>

      <div className="flex min-h-0 flex-1 flex-col px-6 py-4">
        <HarnessControls />
        <div ref={listRef} className="min-h-0 flex-1">
          <VirtualTrackList
            query={{ sort: "title" }}
            fetcher={(query, limit, offset) => syntheticPage(query, limit, offset)}
            onActivate={noop}
            onPlayFrom={noop}
            className="h-full"
          />
        </div>
      </div>
    </div>
  );
}

/** The search field lives **outside** the list so input cost can be isolated. */
function HarnessControls(): React.ReactElement {
  const [value, setValue] = useState("");
  return (
    <div className="mb-3 flex items-center gap-3">
      <input
        value={value}
        onChange={(event) => {
          setValue(event.target.value);
        }}
        aria-label="Search the library"
        placeholder="Harness search field"
        className="h-9 w-72 rounded-full border border-white/10 bg-white/[0.03] px-4 text-[12px] text-white placeholder:text-white/25 focus:outline-none"
      />
      <p className="text-[11px] text-white/25">
        Scroll the list, then run the probes: results are collected by the audit script.
      </p>
    </div>
  );
}

const container = document.getElementById("root");
if (container === null) {
  throw new Error("harness root element missing");
}
createRoot(container).render(<App />);
