import type { JSX } from "react";
import {
  Compass,
  Disc3,
  LibraryBig,
  PanelLeftClose,
  PanelLeftOpen,
  Settings2,
  SlidersHorizontal,
} from "lucide-react";

import { AppLogo } from "@/components/AppLogo";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useUiStore, type ShellRoute } from "@/stores/uiStore";

interface NavItem {
  id: ShellRoute;
  label: string;
  icon: typeof Compass;
  section: "primary" | "secondary";
}

const NAV_ITEMS: readonly NavItem[] = [
  { id: "home", label: "Home", icon: Compass, section: "primary" },
  { id: "library", label: "Library", icon: LibraryBig, section: "primary" },
  { id: "now-playing", label: "Now Playing", icon: Disc3, section: "primary" },
  { id: "settings", label: "Settings", icon: Settings2, section: "secondary" },
  { id: "system", label: "System", icon: SlidersHorizontal, section: "secondary" },
];

/**
 * Navigation rail.
 *
 * The counters under the logo are read from the library store, not from the
 * playback store, so a track change never re-renders the sidebar. Collapsed
 * mode keeps the icon hit-targets at 40 px and moves the accessible name into
 * `title` + `aria-label`, so nothing becomes unreachable by keyboard.
 */
export function Sidebar(): JSX.Element {
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const route = useUiStore((state) => state.route);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const setRoute = useUiStore((state) => state.setRoute);
  const stats = useLibraryStore((state) => state.stats);

  const primary = NAV_ITEMS.filter((item) => item.section === "primary");
  const secondary = NAV_ITEMS.filter((item) => item.section === "secondary");

  const renderItem = (item: NavItem): JSX.Element => {
    const isActive = route === item.id;
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
              Reference player
            </p>
          </div>
        ) : null}
      </div>

      <nav aria-label="Primary" className="flex flex-col gap-1 px-3">
        {primary.map(renderItem)}
      </nav>

      <div className="mx-3 my-4 h-px bg-white/[0.06]" />

      <nav aria-label="Configuration" className="flex flex-col gap-1 px-3">
        {secondary.map(renderItem)}
      </nav>

      <div className={cn("mt-auto space-y-3 px-4 pb-5", collapsed && "px-2")}>
        {!collapsed ? (
          <div className="rounded-2xl border border-white/[0.07] bg-white/[0.025] p-3">
            <p className="text-[9.5px] font-semibold tracking-[0.18em] text-white/25 uppercase">
              Library
            </p>
            <div className="mt-2 space-y-1">
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] text-white/40">Tracks</span>
                <span className="tabular text-[11.5px] text-white/75">
                  {stats === null ? "—" : formatCount(stats.trackCount)}
                </span>
              </div>
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] text-white/40">Albums</span>
                <span className="tabular text-[11.5px] text-white/75">
                  {stats === null ? "—" : formatCount(stats.albumCount)}
                </span>
              </div>
              <div className="flex items-baseline justify-between">
                <span className="text-[11px] text-white/40">Playtime</span>
                <span className="tabular text-[11.5px] text-white/75">
                  {stats === null ? "—" : formatLibraryDuration(stats.totalDurationMs)}
                </span>
              </div>
            </div>
            {stats !== null && stats.missingCount > 0 ? (
              <p className="mt-2 border-t border-white/[0.06] pt-2 text-[10px] text-warning/80">
                {formatCount(stats.missingCount)} missing
              </p>
            ) : null}
          </div>
        ) : (
          <SidebarPulse />
        )}

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

/** Collapsed-state library indicator: a single dot that reflects the transport. */
function SidebarPulse(): JSX.Element {
  const playing = usePlaybackStore(
    (state) => state.status?.state === "playing" || state.status?.state === "buffering",
  );

  return (
    <div className="flex justify-center" title={playing ? "Playing" : "Idle"}>
      <span
        className={cn(
          "h-1.5 w-1.5 rounded-full",
          playing ? "accent-bg animate-pulse" : "bg-white/25",
        )}
        aria-hidden="true"
      />
      <span className="sr-only">{playing ? "Playing" : "Idle"}</span>
    </div>
  );
}
