import { useState, type JSX } from "react";
import { Disc3 } from "lucide-react";

import { coverSrc } from "@/lib/cover";
import { cn } from "@/lib/utils";

interface CoverArtProps {
  /** Absolute path of the cached JPEG, or `null` when there is no artwork. */
  path: string | null;
  /** Lightweight derivative preferred for grids, with `path` as its fallback. */
  thumbnailPath?: string | null;
  /**
   * Remote fallback (the provider URL stored in `cover_cache`). Only used when
   * no local file exists: a library without extracted artwork still shows the
   * cover the catalogue knows about, and a network hiccup never removes a
   * local image.
   */
  url?: string | null;
  alt: string;
  className?: string;
  /** Rendered icon size when the album has no cover. */
  fallbackClassName?: string;
  rounded?: string;
}

/**
 * Cover image with a graceful, deterministic fallback.
 *
 * The fallback is painted underneath the image, so a slow decode never shows a
 * hole and a 404 never leaves a broken-image glyph. `object-cover` plus a fixed
 * aspect ratio on the container means the grid does not move when an image
 * arrives — the CLS budget for this shell is zero.
 */
export function CoverArt({
  path,
  thumbnailPath = null,
  url = null,
  alt,
  className,
  fallbackClassName,
  rounded = "rounded-2xl",
}: CoverArtProps): JSX.Element {
  // Failures are remembered *per URL*, not per component. A virtualised grid
  // reuses the same instance for a different album when a row scrolls away; a
  // boolean flag would then hide the next album's perfectly good cover for the
  // rest of the session. Keyed by source, a retry after the backend attaches
  // artwork also clears itself.
  const [failed, setFailed] = useState<ReadonlySet<string>>(() => new Set());
  const thumbnail = coverSrc(thumbnailPath);
  const local = coverSrc(path);
  const remote = url?.startsWith("https://") === true ? url : null;
  const sources = [...new Set([thumbnail, local, remote].filter((value) => value !== null))];
  const src = sources.find((candidate) => !failed.has(candidate)) ?? null;

  return (
    <div
      className={cn(
        "relative isolate overflow-hidden bg-white/[0.04] ring-1 ring-white/[0.08] ring-inset",
        rounded,
        className,
      )}
    >
      <div
        aria-hidden="true"
        className="absolute inset-0 bg-[radial-gradient(120%_120%_at_20%_0%,rgb(255_255_255/0.10),transparent_60%)]"
      />
      <div className="absolute inset-0 grid place-items-center">
        <Disc3
          aria-hidden="true"
          className={cn("text-white/12", fallbackClassName ?? "h-1/3 w-1/3")}
          strokeWidth={1.25}
        />
      </div>

      {src !== null ? (
        <img
          src={src}
          alt={alt}
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => {
            setFailed((previous) => new Set(previous).add(src));
          }}
          className="absolute inset-0 h-full w-full object-cover"
        />
      ) : null}
    </div>
  );
}
