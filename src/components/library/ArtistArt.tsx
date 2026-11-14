import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState, type JSX } from "react";

import { CoverArt } from "@/components/ui/CoverArt";
import { hasBackend } from "@/lib/cover";
import { cachedPortrait, subscribePortrait } from "@/lib/artistPortraits";
import type { libraryApi } from "@/lib/ipc";
import type { ArtistRow } from "@/types/ipc";

type Portrait = NonNullable<Awaited<ReturnType<typeof libraryApi.artistPortrait>>>;

export function ArtistArt({
  artist,
  className,
  rounded,
  showCredit = false,
}: {
  artist: ArtistRow;
  className: string;
  rounded: string;
  showCredit?: boolean;
}): JSX.Element {
  const ref = useRef<HTMLDivElement>(null);
  const [portrait, setPortrait] = useState<Portrait | null>(() => cachedPortrait(artist.id));
  useEffect(() => {
    const cached = cachedPortrait(artist.id);
    setPortrait(cached);
    if (!hasBackend()) return;
    let alive = true;
    let visible = false;
    let loaded = cached !== null;
    let retried = false;
    let unsubscribe: (() => void) | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const start = (): void => {
      if (!alive || !visible || loaded || unsubscribe !== null) return;
      unsubscribe = subscribePortrait(artist.id, (result) => {
        unsubscribe = null;
        if (!alive || !visible) return;
        if (result !== null) {
          loaded = true;
          setPortrait(result);
          return;
        }
        // Retry once without requiring an app restart. A missing artist/photo
        // is otherwise left alone until the card becomes visible again.
        if (!retried) {
          retried = true;
          timer = setTimeout(start, 120_000);
        }
      });
    };
    const setVisible = (next: boolean): void => {
      visible = next;
      if (next) {
        start();
        return;
      }
      unsubscribe?.();
      unsubscribe = null;
      if (timer !== null) clearTimeout(timer);
      timer = null;
      retried = false;
    };
    const node = ref.current;
    const observer =
      node !== null && typeof IntersectionObserver !== "undefined"
        ? new IntersectionObserver(
            (entries) => {
              setVisible(entries.some((entry) => entry.isIntersecting));
            },
            { rootMargin: "200px" },
          )
        : null;
    if (observer !== null && node !== null) observer.observe(node);
    else setVisible(true);
    return () => {
      alive = false;
      observer?.disconnect();
      unsubscribe?.();
      if (timer !== null) clearTimeout(timer);
    };
  }, [artist.id]);
  const credit =
    portrait === null
      ? ""
      : `${portrait.author.replace(/<[^>]*>/g, "")} · ${portrait.license} · Wikimedia Commons`;
  return (
    <div ref={ref} className={`relative ${className}`} title={credit}>
      <CoverArt
        thumbnailPath={portrait?.path ?? null}
        path={artist.coverPath}
        alt={artist.name}
        className="h-full w-full"
        rounded={rounded}
      />
      {showCredit && portrait !== null ? (
        <button
          type="button"
          className="absolute bottom-1 left-1/2 -translate-x-1/2 rounded bg-black/80 px-2 py-1 text-[9px] text-white"
          title={credit}
          onClick={() => {
            void openUrl(portrait.source);
          }}
        >
          Photo credit
        </button>
      ) : null}
    </div>
  );
}
