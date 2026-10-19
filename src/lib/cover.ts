import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * Artwork URL resolution.
 *
 * The backend stores covers as absolute paths under the application cache;
 * `convertFileSrc` rewrites them to the `asset:` protocol the CSP already
 * allowlists. Keeping this in one place means the `<img>` fallback chain, the
 * player bar and the album grid can never disagree about how a cover is
 * addressed — and a path that arrives unexpectedly (a provider URL, a Windows
 * path with backslashes) degrades to `null` instead of producing a broken image
 * element that the layout has to accommodate.
 */
export function coverSrc(localPath: string | null): string | null {
  if (localPath === null || localPath.length === 0) {
    return null;
  }
  try {
    return convertFileSrc(localPath);
  } catch {
    return null;
  }
}

/** True when the browser can run the Tauri IPC bridge (i.e. not `vite preview`). */
export function hasBackend(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
