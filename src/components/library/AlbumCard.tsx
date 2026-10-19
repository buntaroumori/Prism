import type { JSX } from "react";
import { Play } from "lucide-react";

import { CoverArt } from "@/components/ui/CoverArt";
import { formatCount } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { AlbumRow } from "@/types/ipc";

interface AlbumCardProps {
  album: AlbumRow;
  onPlay: (album: AlbumRow) => void;
  /** True while this album's tracks are being fetched (shows the pressed state). */
  busy?: boolean;
}

/**
 * Album tile used by the home rail.
 *
 * Hover reveals a single action rather than a menu bar: play the album. The
 * cover keeps a fixed square ratio and the text block is two lines with a
 * reserved height, so the rail never reflows as covers and metadata stream in.
 */
export function AlbumCard({ album, onPlay, busy = false }: AlbumCardProps): JSX.Element {
  const meta = [album.year === null ? null : String(album.year), album.quality]
    .filter((value): value is string => value !== null && value.length > 0)
    .join(" · ");

  return (
    <div className="group relative w-[172px] shrink-0">
      <div className="relative">
        <CoverArt
          path={album.coverPath}
          alt={album.title}
          className="aspect-square w-full shadow-[0_18px_40px_-24px_rgb(0_0_0/0.9)] transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] group-hover:-translate-y-0.5 group-hover:scale-[1.015]"
          rounded="rounded-[18px]"
        />

        <button
          type="button"
          onClick={() => {
            onPlay(album);
          }}
          aria-label={`Play ${album.title}`}
          disabled={busy || album.availableTracks === 0}
          className={cn(
            "absolute right-2.5 bottom-2.5 grid h-10 w-10 place-items-center rounded-full bg-white text-oled-950",
            "shadow-[0_10px_30px_-10px_rgb(0_0_0/0.8)] transition-all duration-200",
            "translate-y-1 opacity-0 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100",
            "hover:scale-[1.05] active:scale-[0.96] disabled:pointer-events-none disabled:opacity-40",
            "focus-visible:ring-2 focus-visible:ring-white focus-visible:outline-none",
            busy && "translate-y-0 opacity-100",
          )}
        >
          <Play className="ml-0.5 h-4 w-4" aria-hidden="true" fill="currentColor" strokeWidth={1} />
        </button>
      </div>

      <div className="mt-2.5 min-w-0">
        <p
          className="truncate text-[12.5px] font-medium tracking-tight text-white/90"
          title={album.title}
        >
          {album.title}
        </p>
        <p className="truncate text-[11px] text-white/40" title={album.albumArtist}>
          {album.albumArtist.length > 0 ? album.albumArtist : "Unknown artist"}
        </p>
        <p
          className="truncate text-[10px] text-white/25"
          title={`${String(album.trackCount)} tracks`}
        >
          {meta}
          {album.trackCount > 0 ? ` · ${formatCount(album.trackCount)} tracks` : ""}
        </p>
      </div>
    </div>
  );
}
