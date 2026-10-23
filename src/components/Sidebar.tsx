import { Reorder } from "framer-motion";
import type { JSX } from "react";
import { Compass, Disc3, LibraryBig, ListMusic, PanelLeftClose, PanelLeftOpen } from "lucide-react";

import { AppLogo } from "@/components/AppLogo";
import { CoverArt } from "@/components/ui/CoverArt";
import { cn } from "@/lib/utils";
import { useContextMenuStore } from "@/stores/contextMenuStore";
import { useDockStore, type DockItem } from "@/stores/dockStore";
import { useUiStore, type ShellRoute } from "@/stores/uiStore";

interface NavItem {
  id: ShellRoute;
  label: string;
  icon: typeof Compass;
}

const NAV_ITEMS: readonly NavItem[] = [
  { id: "home", label: "Home", icon: Compass },
  { id: "library", label: "Library", icon: LibraryBig },
  { id: "playlists", label: "Playlists", icon: ListMusic },
  { id: "now-playing", label: "Now Playing", icon: Disc3 },
];

/** Navigation rail and the user's ordered, persistent Dock. */
export function Sidebar(): JSX.Element {
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const route = useUiStore((state) => state.route);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const setRoute = useUiStore((state) => state.setRoute);
  const dock = useDockStore((state) => state.items);
  const reorderDock = useDockStore((state) => state.reorder);

  const renderItem = (item: NavItem): JSX.Element => {
    const isActive =
      route === item.id ||
      (item.id === "playlists" && route === "playlist") ||
      (item.id === "library" && (route === "album" || route === "artist"));
    return (
      <button
        key={item.id}
        type="button"
        onClick={() => {
          setRoute(item.id);
        }}
        aria-current={isActive ? "page" : undefined}
        aria-label={item.label}
        title={collapsed ? item.label : undefined}
        className={cn(
          "group relative flex h-10 items-center gap-3 rounded-xl px-3 text-[13px] font-medium transition-colors duration-150",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
          collapsed && "justify-center px-0",
          isActive
            ? "bg-white/[0.075] text-white shadow-[inset_0_1px_0_rgb(255_255_255/0.06)]"
            : "text-white/45 hover:bg-white/[0.035] hover:text-white/75",
        )}
      >
        <item.icon
          className={cn(
            "h-[18px] w-[18px] shrink-0",
            isActive ? "accent-soft-text" : "text-white/40",
          )}
          aria-hidden="true"
        />
        {!collapsed ? <span className="truncate">{item.label}</span> : null}
        {isActive && collapsed ? (
          <span aria-hidden="true" className="accent-bg absolute left-0 h-5 w-[2px] rounded-full" />
        ) : null}
      </button>
    );
  };

  return (
    <aside
      className={cn(
        "sidebar-glass relative z-20 flex h-full shrink-0 flex-col border-r border-white/[0.07] bg-oled-900/55 backdrop-blur-3xl",
        "transition-[width] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
        collapsed ? "w-[76px]" : "w-[236px]",
      )}
    >
      <div
        className={cn("flex items-center gap-3 px-5 pt-6 pb-5", collapsed && "justify-center px-0")}
      >
        <AppLogo className="h-8 w-8" />
        {!collapsed ? (
          <div className="min-w-0">
            <p className="truncate text-[13px] font-semibold tracking-tight text-white">Prism</p>
            <p className="truncate text-[9px] font-medium tracking-[0.22em] text-white/30 uppercase">
              Your music
            </p>
          </div>
        ) : null}
      </div>

      <nav aria-label="Primary" className="flex shrink-0 flex-col gap-1 px-3">
        {NAV_ITEMS.map(renderItem)}
      </nav>

      <div className="mx-3 my-4 h-px shrink-0 bg-white/[0.06]" />

      <section aria-label="Dock" className="flex min-h-0 flex-1 flex-col">
        {!collapsed ? (
          <h2 className="mb-2 px-5 text-[9.5px] font-semibold tracking-[0.18em] text-white/25 uppercase">
            Dock
          </h2>
        ) : null}
        {dock.length === 0 ? (
          !collapsed ? (
            <p className="px-5 text-[10.5px] leading-relaxed text-white/25">
              Pin albums and playlists from their context menus.
            </p>
          ) : null
        ) : (
          <Reorder.Group
            axis="y"
            values={dock}
            onReorder={reorderDock}
            className="min-h-0 flex-1 space-y-1 overflow-y-auto px-3 pb-2"
          >
            {dock.map((item) => (
              <DockRow key={dockKey(item)} item={item} collapsed={collapsed} />
            ))}
          </Reorder.Group>
        )}
      </section>

      <div className={cn("shrink-0 px-4 pt-2 pb-5", collapsed && "px-2")}>
        <button
          type="button"
          onClick={toggleSidebar}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          aria-expanded={!collapsed}
          className={cn(
            "flex h-9 w-full items-center gap-3 rounded-xl px-3 text-[12px] text-white/35 transition-colors",
            "hover:bg-white/[0.04] hover:text-white/70 focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
            collapsed && "justify-center px-0",
          )}
        >
          {collapsed ? (
            <PanelLeftOpen className="h-4 w-4" aria-hidden="true" />
          ) : (
            <>
              <PanelLeftClose className="h-4 w-4" aria-hidden="true" />
              <span>Collapse</span>
            </>
          )}
        </button>
      </div>
    </aside>
  );
}

function dockKey(item: DockItem): string {
  return item.kind === "album" ? `album:${item.album.id}` : `playlist:${item.playlist.id}`;
}

function DockRow({ item, collapsed }: { item: DockItem; collapsed: boolean }): JSX.Element {
  const openAlbum = useUiStore((state) => state.openAlbum);
  const openPlaylist = useUiStore((state) => state.openPlaylist);
  const openAt = useContextMenuStore((state) => state.openAt);
  const route = useUiStore((state) => state.route);
  const params = useUiStore((state) => state.params);
  const album = item.kind === "album" ? item.album : null;
  const playlist = item.kind === "playlist" ? item.playlist : null;
  const title = album?.title ?? playlist?.name ?? "";
  const secondary = album?.albumArtist ?? "Playlist";
  const path = album?.coverPath ?? playlist?.coverPath ?? null;
  const thumbnailPath = album?.coverThumbnailPath ?? null;
  const url = album?.coverUrl ?? playlist?.coverUrl ?? null;
  const active =
    (album !== null && route === "album" && params.albumId === album.id) ||
    (playlist !== null && route === "playlist" && params.playlistId === playlist.id);

  return (
    <Reorder.Item value={item} className="relative list-none rounded-xl bg-oled-900/95">
      <button
        type="button"
        aria-label={title}
        title={title}
        aria-current={active ? "page" : undefined}
        onClick={() => {
          if (album !== null) openAlbum(album.id);
          if (playlist !== null) openPlaylist(playlist.id);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          if (album !== null) {
            openAt(event.clientX, event.clientY, { kind: "dock-album", album });
          } else if (playlist !== null) {
            openAt(event.clientX, event.clientY, { kind: "dock-playlist", playlist });
          }
        }}
        className={cn(
          "flex w-full items-center gap-3 rounded-xl p-1.5 text-left transition-colors",
          "focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
          collapsed && "justify-center p-1",
          active ? "bg-accent/[0.10]" : "hover:bg-white/[0.045]",
        )}
      >
        <CoverArt
          path={path}
          thumbnailPath={thumbnailPath}
          url={url}
          alt=""
          className={cn("shrink-0", collapsed ? "h-10 w-10" : "h-9 w-9")}
          rounded="rounded-lg"
          fallbackClassName="h-4 w-4"
        />
        {!collapsed ? (
          <span className="min-w-0 flex-1">
            <span
              className={cn(
                "block truncate text-[11.5px]",
                active ? "text-accent" : "text-white/75",
              )}
            >
              {title}
            </span>
            <span className="block truncate text-[10px] text-white/30">{secondary}</span>
          </span>
        ) : null}
      </button>
    </Reorder.Item>
  );
}
