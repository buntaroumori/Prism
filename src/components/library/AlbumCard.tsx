import type { JSX } from "react";
import { Play } from "lucide-react";

import { CoverArt } from "@/components/ui/CoverArt";
import { formatCount } from "@/lib/format";
import { sourceLabel } from "@/lib/sourceLabel";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import type { LibraryContinuation } from "@/stores/playbackStore";
import type { AlbumRow } from "@/types/ipc";

interface AlbumCardProps {
  album: AlbumRow;
  onPlay: (album: AlbumRow) => void;
  /** True while this album's tracks are being fetched (shows the pressed state). */
  busy?: boolean;
  /** Edge length in pixels. Omitted means "size to the parent" (mosaic). */
  size?: number;
  /** Library query shared with Play and the album context menu. */
  continuation?: Omit<LibraryContinuation, "currentAlbumId">;
}

/**
 * Album tile.
 *
 * Two links and one action, exactly like a desktop library: the cover and the
 * title open the album page, the artist line opens the artist page, and the
 * hover button queues the record. The cover keeps a square ratio and the text
 * block reserves its height, so a rail never reflows as artwork streams in.
 */
export function AlbumCard({
  album,
  onPlay,
  busy = false,
  size,
  continuation,
}: AlbumCardProps): JSX.Element {
  const openAlbum = useUiStore((state) => state.openAlbum);
  const openArtist = useUiStore((state) => state.openArtist);
  const openAt = useContextMenuStore((state) => state.openAt);
  const source = sourceLabel({
    format: album.format,
    sampleRate: album.maxSampleRate,
    bitDepth: album.maxBitDepth,
  });

  return (
    <div
      data-interactive
      onContextMenu={(event) => {
        event.preventDefault();
        openAt(event.clientX, event.clientY, {
          kind: "album",
          album,
          ...(continuation === undefined
            ? {}
            : { continuation: { currentAlbumId: album.id, ...continuation } }),
        });
      }}
      className={cn("group relative shrink-0", size === undefined && "w-[172px]")}
      style={size === undefined ? undefined : { width: `${String(size)}px` }}
    >
      <div className="relative">
        <button
          type="button"
          onClick={() => {
            openAlbum(album.id);
          }}
          aria-label={`Open ${album.title}`}
          className="block w-full text-left focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:ring-offset-2 focus-visible:ring-offset-oled-950 focus-visible:outline-none"
        >
          <CoverArt
            path={album.coverPath}
            thumbnailPath={album.coverThumbnailPath}
            url={album.coverUrl}
            alt={album.title}
            className="aspect-square w-full shadow-[0_18px_40px_-24px_rgb(0_0_0/0.9)] transition-transform duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] group-hover:-translate-y-0.5 group-hover:scale-[1.015]"
            rounded="rounded-[18px]"
          />
        </button>

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
        <button
          type="button"
          onClick={() => {
            openAlbum(album.id);
          }}
          data-selectable="album"
          className="block w-full truncate text-left text-[12.5px] font-medium tracking-tight text-white/90 hover:text-white hover:underline hover:decoration-white/30 hover:underline-offset-2"
          title={album.title}
        >
          {album.title}
        </button>
        <button
          type="button"
          onClick={() => {
            if (album.artistId !== null) {
              openArtist(album.artistId);
            }
          }}
          disabled={album.artistId === null}
          data-selectable="artist"
          className="block w-full truncate text-left text-[11px] text-white/40 hover:text-white/75 hover:underline hover:decoration-white/25 hover:underline-offset-2 disabled:hover:text-white/40 disabled:hover:no-underline"
          title={album.albumArtist}
        >
          {album.albumArtist.length > 0 ? album.albumArtist : "Unknown artist"}
        </button>
        <p
          className="truncate text-[10px] text-white/25"
          title={`${String(album.trackCount)} tracks`}
        >
          {album.year !== null ? `${String(album.year)} · ` : ""}
          <span
            className={cn(
              source.tone === "hi-res" && "text-spectrum-violet/75",
              source.tone === "cd" && "text-positive/75",
            )}
          >
            {source.label}
          </span>
          {album.trackCount > 0 ? ` · ${formatCount(album.trackCount)} tracks` : ""}
        </p>
      </div>
    </div>
  );
}
