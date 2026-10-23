import { useEffect, useMemo, useRef, useState, type JSX } from "react";
import { FileAudio, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { activeLineIndex, useLyrics, type LyricsState } from "@/hooks/useLyrics";
import { cn } from "@/lib/utils";
import { playheadMs, usePlaybackStore } from "@/stores/playbackStore";

/** How long auto-follow stays paused after the user scrolls the sheet. */
const FOLLOW_RESUME_MS = 4_000;

/**
 * Synchronised lyrics.
 *
 * The panel follows the playhead line by line, not the other way around: the
 * lyric sheet scrolls itself while the current line is highlighted, and the
 * highlight is driven by the same 8 Hz tick as the scrubber (no extra timer, no
 * polling). Scrolling by hand suspends auto-follow for four seconds so reading
 * ahead does not fight the player.
 *
 * Three real states, all rendered: synced (karaoke cursor), unsynced (plain
 * sheet, no cursor), and no lyrics at all (with a refresh affordance and the
 * exact resolution order, since that is what the user needs to know).
 */
export function LyricsPanel({
  className,
  lyrics,
}: {
  className?: string;
  lyrics?: LyricsState;
}): JSX.Element {
  const status = usePlaybackStore((state) => state.status);
  const tick = usePlaybackStore((state) => state.tick);
  const track = status?.track ?? null;
  const trackId = track?.trackId ?? null;
  const internalLyrics = useLyrics(lyrics === undefined ? trackId : null);
  const { document, loading, error, refresh } = lyrics ?? internalLyrics;
  const position = playheadMs(status, tick);
  const [follow, setFollow] = useState(true);

  const lines = useMemo(() => (document?.synced === true ? document.lines : []), [document]);
  const index = useMemo(() => activeLineIndex(lines, position), [lines, position]);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const lineRefs = useRef(new Map<number, HTMLParagraphElement>());
  const lastScrolled = useRef(-1);
  const resumeTimer = useRef<number | null>(null);

  useEffect(() => {
    const element = lineRefs.current.get(index);
    if (!follow || element === undefined || index === lastScrolled.current) {
      return;
    }
    lastScrolled.current = index;
    const container = containerRef.current;
    if (container === null) {
      return;
    }
    // Manual math instead of `scrollIntoView`: the sheet must move, never the
    // page, and the target line should land at 42 % of the viewport height.
    const target = element.offsetTop - container.clientHeight * 0.42 + element.clientHeight / 2;
    container.scrollTo({
      top: Math.max(0, target),
      behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
    });
  }, [index, follow]);

  useEffect(
    () => () => {
      if (resumeTimer.current !== null) {
        window.clearTimeout(resumeTimer.current);
      }
    },
    [],
  );

  const suspendFollow = (): void => {
    setFollow(false);
    if (resumeTimer.current !== null) {
      window.clearTimeout(resumeTimer.current);
    }
    resumeTimer.current = window.setTimeout(() => {
      setFollow(true);
      lastScrolled.current = -1;
      resumeTimer.current = null;
    }, FOLLOW_RESUME_MS);
  };

  if (track === null) {
    return (
      <Frame className={className}>
        <EmptyState
          title="No track loaded"
          body="Lyrics appear here for whatever the engine is playing."
        />
      </Frame>
    );
  }

  if (loading) {
    return (
      <Frame className={className}>
        <div className="flex h-full items-center justify-center gap-2 text-[12px] text-white/35">
          <RefreshCw className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
          Reading lyrics
        </div>
      </Frame>
    );
  }

  if (error !== null) {
    return (
      <Frame className={className}>
        <EmptyState title="Lyrics could not be read" body={error.message} onRefresh={refresh} />
      </Frame>
    );
  }

  if (document === null || lines.length === 0) {
    if (document !== null && document.plainText.trim().length > 0) {
      return (
        <Frame className={className}>
          <div
            ref={containerRef}
            className="h-full scrollbar-thin overflow-y-auto px-1 py-2"
            onWheel={suspendFollow}
          >
            <p className="mb-3 text-[10px] font-medium tracking-[0.16em] text-white/25 uppercase">
              Unsynced · {document.provider}
            </p>
            <div className="space-y-2.5">
              {document.plainText.split("\n").map((line, position2) => (
                <p
                  key={`${String(position2)}-${line.slice(0, 12)}`}
                  className="text-[13.5px] leading-relaxed text-white/60"
                >
                  {line.length > 0 ? line : "\u00a0"}
                </p>
              ))}
            </div>
          </div>
        </Frame>
      );
    }
    return (
      <Frame className={className}>
        <EmptyState
          title="No lyrics for this track"
          body="Prism reads an embedded USLT frame first, then a .lrc file beside the audio file. Add one and refresh."
          onRefresh={refresh}
        />
      </Frame>
    );
  }

  return (
    <Frame className={className}>
      <div className="flex items-center justify-between px-1 pb-2">
        <p className="text-[10px] font-medium tracking-[0.16em] text-white/25 uppercase">
          {document.synced ? "Synced" : "Plain"} · {document.provider}
          {document.language !== null ? ` · ${document.language}` : ""}
        </p>
        <button
          type="button"
          onClick={refresh}
          aria-label="Re-read lyrics from disk"
          className="grid h-6 w-6 place-items-center rounded-full text-white/25 transition-colors hover:bg-white/[0.06] hover:text-white/70"
        >
          <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </div>

      <div
        ref={containerRef}
        onWheel={suspendFollow}
        onTouchMove={suspendFollow}
        className="relative h-full scrollbar-thin overflow-y-auto pr-2 pb-16"
      >
        {document.offsetMs !== 0 ? (
          <p className="mb-2 text-[10px] text-white/20">
            Offset {document.offsetMs > 0 ? "+" : ""}
            {String(document.offsetMs)} ms
          </p>
        ) : null}
        {lines.map((line, lineIndex) => {
          const isActive = lineIndex === index;
          const isPast = lineIndex < index;
          return (
            <p
              key={`${String(line.timeMs)}-${String(lineIndex)}`}
              ref={(element) => {
                if (element === null) {
                  lineRefs.current.delete(lineIndex);
                } else {
                  lineRefs.current.set(lineIndex, element);
                }
              }}
              className={cn(
                "py-1.5 text-[19px] leading-snug font-medium tracking-[-0.01em] transition-[color,opacity,transform] duration-200 ease-out",
                isActive
                  ? "text-white"
                  : isPast
                    ? "text-white/25"
                    : "text-white/45 hover:text-white/70",
              )}
              style={
                isActive
                  ? {
                      textShadow:
                        "0 0 28px color-mix(in srgb, var(--prism-accent) 45%, transparent)",
                    }
                  : undefined
              }
            >
              {line.text.length > 0 ? line.text : "\u00a0"}
            </p>
          );
        })}
      </div>
    </Frame>
  );
}

function Frame({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string | undefined;
}): JSX.Element {
  return (
    <div
      className={cn(
        "relative h-full overflow-hidden rounded-3xl border border-white/[0.06] bg-white/[0.02] backdrop-blur-xl",
        className,
      )}
    >
      {children}
    </div>
  );
}

function EmptyState({
  title,
  body,
  onRefresh,
}: {
  title: string;
  body: string;
  onRefresh?: () => void;
}): JSX.Element {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6 text-center">
      <div className="grid h-11 w-11 place-items-center rounded-full border border-white/[0.07] bg-white/[0.03]">
        <FileAudio className="h-4.5 w-4.5 text-white/25" aria-hidden="true" strokeWidth={1.5} />
      </div>
      <p className="mt-3 text-[13px] font-medium text-white/65">{title}</p>
      <p className="mt-1.5 max-w-xs text-[11.5px] leading-relaxed text-white/30">{body}</p>
      {onRefresh !== undefined ? (
        <Button variant="ghost" icon={RefreshCw} className="mt-4 h-8" onClick={onRefresh}>
          Refresh
        </Button>
      ) : null}
    </div>
  );
}
