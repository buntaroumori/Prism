import type { JSX } from "react";
import {
  Activity,
  Database,
  Gauge,
  LibraryBig,
  PanelLeftClose,
  PanelLeftOpen,
  type LucideIcon,
} from "lucide-react";

import { AppLogo } from "@/components/AppLogo";
import { StatusPill } from "@/components/ui/StatusPill";
import { cn } from "@/lib/utils";
import { useUiStore, type ShellSection } from "@/stores/uiStore";
import type { AppInfo, RuntimeHealth } from "@/types/ipc";

interface NavItem {
  id: ShellSection;
  label: string;
  icon: LucideIcon;
}

const NAV_ITEMS: readonly NavItem[] = [
  { id: "overview", label: "Overview", icon: Gauge },
  { id: "library", label: "Library", icon: LibraryBig },
  { id: "database", label: "Database", icon: Database },
  { id: "runtime", label: "Runtime", icon: Activity },
];

interface SidebarProps {
  appInfo: AppInfo | null;
  healthStatus: RuntimeHealth["status"] | null;
}

/**
 * Persistent navigation rail.
 *
 * Sections are in-page anchors: clicking one scrolls the main scroller and
 * records the active section in the persisted shell store. The rail collapses
 * to a 76 px icon strip; labels are hidden but remain available as `title`
 * tooltips and the accessible name never disappears.
 */
export function Sidebar({ appInfo, healthStatus }: SidebarProps): JSX.Element {
  const collapsed = useUiStore((state) => state.sidebarCollapsed);
  const activeSection = useUiStore((state) => state.activeSection);
  const toggleSidebar = useUiStore((state) => state.toggleSidebar);
  const setActiveSection = useUiStore((state) => state.setActiveSection);

  const handleNavigate = (section: ShellSection): void => {
    setActiveSection(section);
    const target = document.getElementById(section);
    target?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const statusTone =
    healthStatus === "ok" ? "positive" : healthStatus === "degraded" ? "warning" : "neutral";
  const statusLabel =
    healthStatus === "ok" ? "Engine online" : healthStatus === "degraded" ? "Degraded" : "Starting";

  return (
    <aside
      className={cn(
        "relative z-20 flex h-full shrink-0 flex-col border-r border-white/[0.07] bg-oled-900/50 backdrop-blur-3xl",
        "transition-[width] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)]",
        collapsed ? "w-[76px]" : "w-[248px]",
      )}
    >
      <div className={cn("flex items-center gap-3 px-5 pt-6", collapsed && "justify-center px-0")}>
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

      <nav aria-label="Sections" className="mt-9 flex flex-col gap-1 px-3">
        {NAV_ITEMS.map((item) => {
          const isActive = activeSection === item.id;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => {
                handleNavigate(item.id);
              }}
              aria-current={isActive ? "page" : undefined}
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
                  isActive ? "text-accent-soft" : "text-white/40",
                )}
                aria-hidden="true"
              />
              {!collapsed ? <span className="truncate">{item.label}</span> : null}
              {isActive && !collapsed ? (
                <span
                  aria-hidden="true"
                  className="absolute top-1/2 right-3 h-1 w-1 -translate-y-1/2 rounded-full bg-accent"
                />
              ) : null}
            </button>
          );
        })}
      </nav>

      <div className={cn("mt-auto space-y-4 px-4 pb-5", collapsed && "px-2")}>
        {!collapsed ? (
          <div className="rounded-2xl border border-white/[0.07] bg-white/[0.025] p-3">
            <StatusPill tone={statusTone} pulse={healthStatus === "ok"}>
              {statusLabel}
            </StatusPill>
            <p
              className="mt-2.5 truncate font-mono text-[10px] text-white/30"
              title={appInfo?.databasePath ?? "Resolving library database…"}
            >
              {appInfo?.databasePath ?? "Resolving database…"}
            </p>
            <p className="mt-1 text-[10px] text-white/25">
              {appInfo !== null ? `v${appInfo.version} · ${appInfo.buildProfile}` : "v0.1.0"}
            </p>
          </div>
        ) : (
          <div className="flex justify-center">
            <span
              className={cn(
                "h-2 w-2 rounded-full",
                healthStatus === "ok"
                  ? "bg-positive"
                  : healthStatus === "degraded"
                    ? "bg-warning"
                    : "bg-white/30",
              )}
              aria-hidden="true"
            />
          </div>
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
