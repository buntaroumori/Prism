import { useState, type JSX } from "react";
import { Disc3 } from "lucide-react";

import { coverSrc } from "@/lib/cover";
import { cn } from "@/lib/utils";

interface CoverArtProps {
  /** Absolute path of the cached JPEG, or `null` when there is no artwork. */
  path: string | null;
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
  alt,
  className,
  fallbackClassName,
  rounded = "rounded-2xl",
}: CoverArtProps): JSX.Element {
  const [failed, setFailed] = useState(false);
  const src = coverSrc(path);

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

      {src !== null && !failed ? (
        <img
          src={src}
          alt={alt}
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => {
            setFailed(true);
          }}
          className="absolute inset-0 h-full w-full object-cover"
        />
      ) : null}
    </div>
  );
}
