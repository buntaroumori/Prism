import { ChevronRight, Loader2, Maximize2, Minimize2, Search, Settings, X } from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type JSX,
  type MouseEvent as ReactMouseEvent,
} from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { WindowControls } from "@/components/WindowControls";
import { hasBackend } from "@/lib/cover";
import { formatCount } from "@/lib/format";
import { libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore, type ShellRoute } from "@/stores/uiStore";
import { useWindowStore } from "@/stores/windowStore";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import type { GlobalSearch } from "@/types/ipc";

const ROUTE_LABELS: Record<ShellRoute, string> = {
  home: "Home",
  library: "Library",
  collection: "Collection",
  playlists: "Playlists",
  playlist: "Playlist",
  "now-playing": "Now Playing",
  queue: "Queue",
  settings: "Settings",
  album: "Album",
  artist: "Artist",
};

/**
 * Window chrome, page context and the one search field in the product.
 *
 * The search sits in the centre of the bar and is present on every route, so
 * there is no view where the user has to remember a different shortcut. Typing
 * queries the backend's global search; Enter filters the library mosaic, and an
 * individual result opens its album, its artist, or plays the track.
 *
 * The scan is *not* here: starting a scan belongs in Settings → Library, where
 * the roots and the history live. A bar that can kick off a full-disk read on
 * every screen is a mistake waiting for a misplaced click.
 */
export function TopBar(): JSX.Element {
  const route = useUiStore((state) => state.route);
  const navigate = useUiStore((state) => state.navigate);
  const toggleSettings = useUiStore((state) => state.toggleSettings);
  const openAlbum = useUiStore((state) => state.openAlbum);
  const openArtist = useUiStore((state) => state.openArtist);
  const setQuery = useLibraryStore((state) => state.setQuery);
  const scan = useLibraryStore((state) => state.scan);
  const progress = useLibraryStore((state) => state.progress);
  const playback = usePlaybackStore((state) => state.playTracks);
  const fullscreen = useWindowStore((state) => state.fullscreen);
  const toggleFullscreen = useWindowStore((state) => state.toggleFullscreen);
  const openAt = useContextMenuStore((state) => state.openAt);

  const [text, setText] = useState("");
  const [results, setResults] = useState<GlobalSearch | null>(null);
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const previousRoute = useRef(route);

  useEffect(() => {
    if (previousRoute.current === "library" && route !== "library") {
      setText("");
      setResults(null);
      setOpen(false);
    }
    previousRoute.current = route;
  }, [route]);

  const running = scan?.running === true || progress !== null;
  const label = ROUTE_LABELS[route];

  // Cmd/Ctrl+K from anywhere: the shortcut every desktop search field has.
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
      if (event.key === "Escape") {
        setOpen(false);
        inputRef.current?.blur();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  useEffect(() => {
    const trimmed = text.trim();
    if (trimmed.length < 2) {
      setResults(null);
      return;
    }
    let active = true;
    const timer = window.setTimeout(() => {
      void libraryApi
        .globalSearch(trimmed, 12)
        .then((next) => {
          if (active) {
            setResults(next);
          }
        })
        .catch(() => {
          if (active) {
            setResults(null);
          }
        });
    }, 180);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [text]);

  // A click outside the field closes the panel; the field keeps its text.
  useEffect(() => {
    const onDown = (event: MouseEvent): void => {
      if (boxRef.current !== null && !boxRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("mousedown", onDown);
    };
  }, []);

  const submit = useCallback((): void => {
    const trimmed = text.trim();
    setQuery(trimmed);
    setOpen(false);
    navigate("library");
  }, [navigate, setQuery, text]);

  return (
    <header
      data-topbar
      onMouseDown={(event) => {
        const target = event.target as Element | null;
        if (
          event.button !== 0 ||
          !hasBackend() ||
          target?.closest(
            "button, input, textarea, select, [role='button'], [role='listbox'], [data-interactive], .no-drag",
          ) !== null
        ) {
          return;
        }
        event.preventDefault();
        void getCurrentWindow()
          .startDragging()
          .catch((cause: unknown) => {
            console.warn("Window dragging could not start", cause);
          });
      }}
      className="relative z-20 flex h-[62px] shrink-0 items-center gap-4 border-b border-white/[0.05] px-7"
    >
      <div className="flex w-[210px] min-w-0 shrink-0 items-center gap-2 text-[12.5px]">
        <span className="text-white/30">Prism</span>
        <ChevronRight className="h-3.5 w-3.5 text-white/15" aria-hidden="true" />
        <span className="truncate font-medium tracking-tight text-white/80">{label}</span>
      </div>

      <div ref={boxRef} className="no-drag relative flex min-w-0 flex-1 justify-center">
        <div
          className={cn(
            "flex h-9 w-full max-w-[520px] items-center rounded-full border bg-white/[0.035] px-3.5 transition-colors",
            open && text.trim().length >= 2
              ? "border-accent/40 bg-white/[0.05]"
              : "border-white/[0.08] hover:border-white/[0.14]",
          )}
        >
          <Search className="h-3.5 w-3.5 shrink-0 text-white/30" aria-hidden="true" />
          <input
            ref={inputRef}
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              setOpen(true);
            }}
            onFocus={() => {
              setOpen(true);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                submit();
              }
            }}
            placeholder="Search albums, artists, tracks"
            aria-label="Search the library"
            className="ml-2.5 h-full min-w-0 flex-1 bg-transparent text-[12.5px] text-white placeholder:text-white/25 focus:outline-none"
          />
          {text.length > 0 ? (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => {
                setText("");
                setQuery("");
                setResults(null);
                inputRef.current?.focus();
              }}
              className="grid h-5 w-5 place-items-center rounded-full text-white/30 hover:bg-white/[0.08] hover:text-white/80"
            >
              <X className="h-3 w-3" aria-hidden="true" />
            </button>
          ) : (
            <kbd className="hidden shrink-0 rounded border border-white/10 px-1.5 py-0.5 text-[9.5px] text-white/25 sm:block">
              Ctrl K
            </kbd>
          )}
        </div>

        {open && text.trim().length >= 2 ? (
          <div className="absolute top-[46px] left-1/2 w-full max-w-[520px] -translate-x-1/2 overflow-hidden rounded-2xl border border-white/[0.09] bg-oled-950/95 shadow-[0_30px_80px_-20px_rgb(0_0_0/0.9)] backdrop-blur-2xl">
            {results === null ? (
              <p className="px-4 py-3 text-[11.5px] text-white/35">Searching…</p>
            ) : results.albums.length === 0 &&
              results.artists.length === 0 &&
              results.tracks.items.length === 0 ? (
              <p className="px-4 py-3 text-[11.5px] text-white/35">
                Nothing matches “{text.trim()}”.
              </p>
            ) : (
              <div className="max-h-[420px] overflow-y-auto py-1.5">
                <Group title="Albums">
                  {results.albums.slice(0, 5).map((album) => (
                    <Row
                      key={album.id}
                      primary={album.title}
                      secondary={album.albumArtist}
                      onPick={() => {
                        openAlbum(album.id);
                        setOpen(false);
                      }}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        openAt(event.clientX, event.clientY, { kind: "album", album });
                      }}
                    />
                  ))}
                </Group>
                <Group title="Artists">
                  {results.artists.slice(0, 4).map((artist) => (
                    <Row
                      key={artist.id}
                      primary={artist.name}
                      secondary={`${formatCount(artist.albumCount)} albums`}
                      onPick={() => {
                        openArtist(artist.id);
                        setOpen(false);
                      }}
                    />
                  ))}
                </Group>
                <Group title="Tracks">
                  {results.tracks.items.slice(0, 5).map((track) => (
                    <Row
                      key={track.id}
                      primary={track.title}
                      secondary={`${track.artist} · ${track.album}`}
                      onPick={() => {
                        void playback([track], 0);
                        setOpen(false);
                      }}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        openAt(event.clientX, event.clientY, { kind: "track", track });
                      }}
                    />
                  ))}
                </Group>
                <button
                  type="button"
                  onClick={submit}
                  className="mt-1 w-full border-t border-white/[0.06] px-4 py-2.5 text-left text-[11.5px] text-white/45 hover:bg-white/[0.03] hover:text-white"
                >
                  Show all results in the library
                </button>
              </div>
            )}
          </div>
        ) : null}
      </div>

      <div className="flex w-[360px] shrink-0 items-center justify-end gap-1.5">
        {running ? (
          <div
            className="mr-1 flex items-center gap-1.5 text-white/45"
            title="Library scan in progress"
          >
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            <span className="text-[10.5px] whitespace-nowrap">
              {progress === null ? "Scanning" : formatCount(progress.filesIndexed)}
            </span>
          </div>
        ) : null}
        <button
          type="button"
          aria-label={fullscreen ? "Exit fullscreen" : "Enter fullscreen"}
          title={fullscreen ? "Exit fullscreen (F11)" : "Fullscreen (F11)"}
          onClick={() => {
            void toggleFullscreen();
          }}
          className="grid h-8 w-8 shrink-0 place-items-center rounded-full text-white/35 transition-colors hover:bg-white/[0.06] hover:text-white"
        >
          {fullscreen ? (
            <Minimize2 className="h-3.5 w-3.5" aria-hidden="true" />
          ) : (
            <Maximize2 className="h-3.5 w-3.5" aria-hidden="true" />
          )}
        </button>
        <button
          type="button"
          aria-label="Settings"
          title="Settings"
          aria-current={route === "settings" ? "page" : undefined}
          onClick={toggleSettings}
          className={
            route === "settings"
              ? "grid h-8 w-8 place-items-center rounded-full bg-accent/[0.12] text-accent"
              : "grid h-8 w-8 place-items-center rounded-full text-white/35 transition-colors hover:bg-white/[0.06] hover:text-white"
          }
        >
          <Settings className="h-4 w-4" aria-hidden="true" />
        </button>
        <WindowControls />
      </div>
    </header>
  );
}

function Group({
  title,
  children,
}: {
  title: string;
  children: JSX.Element[];
}): JSX.Element | null {
  if (children.length === 0) {
    return null;
  }
  return (
    <div className="px-1.5 py-1">
      <p className="px-2.5 pt-1 pb-1 text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase">
        {title}
      </p>
      {children}
    </div>
  );
}

function Row({
  primary,
  secondary,
  onPick,
  onContextMenu,
}: {
  primary: string;
  secondary: string;
  onPick: () => void;
  onContextMenu?: (event: ReactMouseEvent<HTMLButtonElement>) => void;
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={onPick}
      onContextMenu={onContextMenu}
      className="flex w-full min-w-0 items-center gap-2 rounded-xl px-2.5 py-1.5 text-left hover:bg-white/[0.05]"
    >
      <span className="truncate text-[12.5px] text-white/85">{primary}</span>
      <span className="ml-auto truncate text-[10.5px] text-white/30">{secondary}</span>
    </button>
  );
}
