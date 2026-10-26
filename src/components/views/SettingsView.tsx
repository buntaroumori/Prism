import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState, type JSX, type ReactNode } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  FolderOpen,
  FolderPlus,
  FolderX,
  Loader2,
  Monitor,
  Moon,
  Power,
  RefreshCw,
  Sun,
  Trash2,
} from "lucide-react";

import { PluginsSection } from "@/components/settings/PluginsSection";
import { SystemView } from "@/components/views/SystemView";
import { Button } from "@/components/ui/Button";
import { PopoverSelect } from "@/components/ui/PopoverSelect";
import { Segmented } from "@/components/ui/Segmented";
import { StatusPill } from "@/components/ui/StatusPill";
import { formatAudioFormat } from "@/lib/audio";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { discordApi, libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { usePlaybackPreferencesStore } from "@/stores/playbackPreferencesStore";
import { useOutputStore, BUFFER_CHOICES } from "@/stores/outputStore";
import { COVER_SIZE_MAX, COVER_SIZE_MIN, useUiStore, type ThemePreference } from "@/stores/uiStore";
import type { DiscordStatus, LibraryRoot } from "@/types/ipc";

/**
 * Settings.
 *
 * Four sections, each backed by a real command: output/engine, library folders,
 * scanning policy and Discord presence. Nothing here is a preview — pressing a
 * control changes the running process, and the resulting state is read back
 * from the backend rather than assumed.
 */
export function SettingsView(): JSX.Element {
  return (
    <div className="mx-auto w-full max-w-[980px] space-y-10 px-8 pt-8 pb-14">
      <header>
        <h1 className="text-[27px] font-semibold tracking-[-0.03em] text-white">Settings</h1>
        <p className="mt-1 text-[13px] text-white/40">
          Your choices are saved and applied to this installation.
        </p>
      </header>

      <ThemeSection />
      <OutputSection />
      <PlaybackSection />
      <LibrarySection />
      <ScanSection />
      <PresenceSection />
      <PluginsSection />
      <AppearanceSection />
      <SystemView embedded />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

const THEME_OPTIONS = [
  { value: "light", label: "Light", caption: "Fixed light", icon: Sun },
  { value: "dark", label: "Dark", caption: "Fixed dark", icon: Moon },
  { value: "system", label: "System", caption: "Light / dark", icon: Monitor },
] as const;

function ThemeSection(): JSX.Element {
  const theme = useUiStore((state) => state.theme);
  const setAppearance = useUiStore((state) => state.setAppearance);

  return (
    <Section
      title="Theme"
      description="Choose a fixed appearance or follow the operating system. Your choice is applied immediately and remembered."
    >
      <div role="radiogroup" aria-label="Theme" className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        {THEME_OPTIONS.map((option) => {
          const selected = theme === option.value;
          const Icon = option.icon;
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={selected}
              aria-label={option.label}
              data-theme-choice={option.value}
              onClick={() => {
                setAppearance({ theme: option.value });
              }}
              className={cn(
                "rounded-2xl border p-3 text-left transition-colors focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
                selected
                  ? "border-accent/45 bg-accent/[0.07]"
                  : "border-white/[0.07] bg-white/[0.02] hover:border-white/[0.14] hover:bg-white/[0.04]",
              )}
            >
              <HomePreview theme={option.value} />
              <span className="mt-3 flex items-center gap-2">
                <Icon
                  className={cn("h-4 w-4", selected ? "text-accent" : "text-white/40")}
                  aria-hidden="true"
                />
                <span className={cn("text-[12.5px] font-medium", selected && "text-accent")}>
                  {option.label}
                </span>
                <span className="ml-auto text-[9.5px] text-white/25">{option.caption}</span>
              </span>
            </button>
          );
        })}
      </div>
    </Section>
  );
}

function HomePreview({ theme }: { theme: ThemePreference }): JSX.Element {
  if (theme === "system") {
    return (
      <div aria-hidden="true" className="grid grid-cols-2 overflow-hidden rounded-xl">
        <MiniHome dark />
        <MiniHome dark={false} />
      </div>
    );
  }
  return <MiniHome dark={theme === "dark"} />;
}

/** Symbolic Home silhouette: rail, search, shelf labels and album covers. */
function MiniHome({ dark }: { dark: boolean }): JSX.Element {
  const base = dark ? "#08080a" : "#f4f4f6";
  const panel = dark ? "#17171b" : "#ffffff";
  const line = dark ? "rgba(255,255,255,.16)" : "rgba(28,28,32,.16)";
  const faint = dark ? "rgba(255,255,255,.07)" : "rgba(28,28,32,.07)";
  return (
    <div
      aria-hidden="true"
      className="flex h-[68px] min-w-0 overflow-hidden rounded-xl border"
      style={{ background: base, borderColor: line }}
    >
      <div
        className="flex w-[22%] shrink-0 flex-col gap-1.5 border-r p-2"
        style={{ borderColor: line }}
      >
        <span className="h-2 w-2 rounded-full bg-accent" />
        <span className="mt-1 h-1 rounded-full" style={{ background: line }} />
        <span className="h-1 w-3/4 rounded-full" style={{ background: faint }} />
        <span className="h-1 w-4/5 rounded-full" style={{ background: faint }} />
      </div>
      <div className="min-w-0 flex-1 p-2">
        <div className="mx-auto h-2 w-3/5 rounded-full" style={{ background: panel }} />
        <div className="mt-2 h-1 w-2/5 rounded-full" style={{ background: line }} />
        <div className="mt-1.5 grid grid-cols-3 gap-1">
          {[0, 1, 2].map((item) => (
            <span
              key={item}
              className="aspect-square rounded-[3px]"
              style={{ background: faint }}
            />
          ))}
        </div>
        <div className="mt-1.5 h-1 w-1/3 rounded-full" style={{ background: line }} />
        <div className="mt-1 grid grid-cols-3 gap-1">
          {[0, 1, 2].map((item) => (
            <span key={item} className="h-2 rounded-[2px]" style={{ background: faint }} />
          ))}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

function OutputSection(): JSX.Element {
  const bufferFrames = useOutputStore((state) => state.bufferFrames);
  const deviceId = useOutputStore((state) => state.deviceId);
  const devices = useOutputStore((state) => state.devices);
  const deviceFormat = useOutputStore((state) => state.deviceFormat);
  const setDevice = useOutputStore((state) => state.setDevice);
  const setBufferFrames = useOutputStore((state) => state.setBufferFrames);
  const ratePolicy = useOutputStore((state) => state.ratePolicy);
  const setRatePolicy = useOutputStore((state) => state.setRatePolicy);
  const loadDevices = useOutputStore((state) => state.loadDevices);
  const status = usePlaybackStore((state) => state.status);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  const live = deviceFormat ?? status?.deviceFormat ?? null;
  const latencyMs = live === null ? null : (bufferFrames / live.sampleRate) * 1_000;

  return (
    <Section
      title="Output"
      description="Choose where your music plays and how Prism handles songs with different sample rates."
    >
      <Row
        label="Output device"
        hint="Speakers, headphones and other playback devices available on this computer."
      >
        <div className="flex min-w-0 items-center gap-2">
          <PopoverSelect
            ariaLabel="Output device"
            value={deviceId ?? ""}
            onChange={(value) => {
              void setDevice(value.length === 0 ? null : value);
            }}
            options={[
              { value: "", label: "System default" },
              ...devices.map((device) => ({ value: device.id, label: device.name })),
            ]}
            buttonClassName="w-[280px] max-w-[320px]"
            menuClassName="w-[340px] max-w-[80vw] whitespace-normal"
          />
          <Button
            variant="ghost"
            icon={RefreshCw}
            onClick={() => {
              void loadDevices();
            }}
            aria-label="Rescan devices"
            className="h-8 w-8 rounded-full px-0"
          />
        </div>
      </Row>

      <Row
        label="Device configuration"
        hint="The format currently reported by your playback device."
      >
        <div className="flex items-center gap-2">
          <span className="tabular rounded-lg border border-white/[0.08] bg-white/[0.03] px-2.5 py-1 text-[11.5px] text-white/70">
            {live === null ? "Reading…" : formatAudioFormat(live)}
          </span>
          <span className="text-[10.5px] text-white/25">
            {live === null ? "" : `${String(live.channels)} ch`}
          </span>
        </div>
      </Row>

      <Row
        label="Sample rate"
        hint={
          ratePolicy === "matchSource"
            ? "Each song plays at its own sample rate when your device allows it. Otherwise, Prism converts it automatically."
            : "Your device keeps one sample rate and every song is converted to match it."
        }
      >
        <Segmented
          label="Sample rate policy"
          layoutId="settings-rate-policy"
          value={ratePolicy}
          onChange={(next) => {
            void setRatePolicy(next === "fixed" ? "fixed" : "matchSource");
          }}
          options={[
            { value: "matchSource", label: "Follow the record" },
            { value: "fixed", label: "Device rate" },
          ]}
        />
      </Row>

      <Row label="Buffer" hint={bufferHint(bufferFrames, latencyMs)}>
        <Segmented
          label="Buffer size"
          layoutId="settings-buffer"
          value={String(bufferFrames)}
          onChange={(next) => {
            void setBufferFrames(Number(next));
          }}
          options={BUFFER_CHOICES.map((value) => ({
            value: String(value),
            label: String(value),
          }))}
        />
      </Row>

      <p className="rounded-xl border border-white/[0.06] bg-white/[0.015] px-3.5 py-2.5 text-[11px] leading-relaxed text-white/35">
        {bufferFrames <= 128
          ? "The shortest response time, best for a computer with a stable audio connection. If you hear interruptions, choose a larger buffer."
          : bufferFrames <= 512
            ? "A balance between responsive controls and uninterrupted playback. This is the best choice for most systems."
            : "The most resilient option for slow drives or network folders. Controls may take a moment longer to respond."}
      </p>
    </Section>
  );
}

/** One sentence that describes what the chosen buffer actually does. */
function bufferHint(frames: number, latencyMs: number | null): string {
  const ms = latencyMs === null ? null : `${latencyMs.toFixed(1)} ms`;
  const shape =
    frames <= 128
      ? "Lowest latency, least tolerance for load."
      : frames <= 512
        ? "Balanced latency and resilience."
        : "Maximum resilience, higher latency.";
  return ms === null ? shape : `${shape} One period is ${ms} at the device rate.`;
}

// ---------------------------------------------------------------------------
// Playback behaviour
// ---------------------------------------------------------------------------

function PlaybackSection(): JSX.Element {
  const albumEnd = usePlaybackPreferencesStore((state) => state.albumEnd);
  const setAlbumEnd = usePlaybackPreferencesStore((state) => state.setAlbumEnd);
  return (
    <Section
      title="Playback"
      description="Choose what happens when an album started from Library reaches its final song."
    >
      <Row
        label="At album end"
        hint="Continue uses the originating Library filters and sort order, including full release dates."
      >
        <Segmented
          label="Behaviour at album end"
          layoutId="settings-album-end"
          value={albumEnd}
          onChange={(next) => {
            setAlbumEnd(next === "continue-library" ? "continue-library" : "stop");
          }}
          options={[
            { value: "stop", label: "Stop at album end" },
            { value: "continue-library", label: "Continue in library order" },
          ]}
        />
      </Row>
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Library roots
// ---------------------------------------------------------------------------

function LibrarySection(): JSX.Element {
  const roots = useLibraryStore((state) => state.roots);
  const addRoot = useLibraryStore((state) => state.addRoot);
  const refreshRoots = useLibraryStore((state) => state.loadRoots);
  const setRootEnabled = useLibraryStore((state) => state.setRootEnabled);
  const error = useLibraryStore((state) => state.error);
  const stats = useLibraryStore((state) => state.stats);
  const refreshStats = useLibraryStore((state) => state.refreshStats);
  const [picking, setPicking] = useState(false);
  const [removal, setRemoval] = useState<string | null>(null);

  const purge = async (id: string, name: string): Promise<void> => {
    try {
      const outcome = await libraryApi.removeRoot(id);
      setRemoval(
        `${name}: ${formatCount(outcome.tracksRemoved)} tracks, ${formatCount(outcome.albumsRemoved)} albums and ${formatCount(outcome.artistsRemoved)} artists removed from the catalogue`,
      );
      await Promise.all([refreshRoots(), refreshStats()]);
    } catch (cause) {
      setRemoval(cause instanceof Error ? cause.message : "The folder could not be removed");
    }
  };

  const pickFolder = async (): Promise<void> => {
    setPicking(true);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Add a music folder",
      });
      if (typeof selected === "string") {
        await addRoot(selected);
      }
    } finally {
      setPicking(false);
    }
  };

  return (
    <Section
      title="Library folders"
      description="Prism finds music in these folders and the folders inside them. Removing a folder only removes its music from Prism; your files are not deleted."
    >
      <div className="space-y-2">
        {roots.length === 0 ? (
          <p className="rounded-xl border border-dashed border-white/[0.09] px-4 py-6 text-center text-[12.5px] text-white/35">
            No folders yet. Add the directory that holds your music.
          </p>
        ) : (
          roots.map((root) => (
            <RootRow
              key={root.id}
              root={root}
              onToggle={(enabled) => {
                void setRootEnabled(root.id, enabled);
              }}
              onRemove={() => {
                void purge(root.id, root.name.length > 0 ? root.name : root.path);
              }}
            />
          ))
        )}

        <div className="flex items-center gap-2 pt-1">
          <Button
            variant="secondary"
            icon={FolderPlus}
            loading={picking}
            onClick={() => {
              void pickFolder();
            }}
          >
            Add folder
          </Button>
          {stats !== null ? (
            <span className="text-[11.5px] text-white/30">
              {formatCount(stats.trackCount)} tracks ·{" "}
              {formatLibraryDuration(stats.totalDurationMs)}
            </span>
          ) : null}
        </div>

        {removal !== null ? (
          <p
            role="status"
            className="rounded-xl bg-white/[0.03] px-3 py-2 text-[11.5px] text-white/50"
          >
            {removal}
          </p>
        ) : null}

        {error !== null ? (
          <p
            role="alert"
            className="rounded-xl bg-critical/[0.08] px-3 py-2 text-[11.5px] text-critical"
          >
            {error.message}
          </p>
        ) : null}
      </div>
    </Section>
  );
}

function RootRow({
  root,
  onToggle,
  onRemove,
}: {
  root: LibraryRoot;
  onToggle: (enabled: boolean) => void;
  onRemove: () => void;
}): JSX.Element {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-white/[0.07] bg-white/[0.02] px-3.5 py-2.5">
      <span
        aria-hidden="true"
        className={cn(
          "h-1.5 w-1.5 shrink-0 rounded-full",
          !root.available ? "bg-critical" : root.enabled ? "bg-positive" : "bg-white/25",
        )}
      />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12.5px] text-white/85" title={root.path}>
          {root.name.length > 0 ? root.name : root.path}
        </p>
        <p className="truncate font-mono text-[10px] text-white/25" title={root.path}>
          {root.path}
        </p>
      </div>

      <span className="tabular shrink-0 text-[11px] text-white/35">{formatCount(root.tracks)}</span>

      {!root.available ? (
        <StatusPill tone="critical">
          <FolderX className="h-3 w-3" aria-hidden="true" /> Unreachable
        </StatusPill>
      ) : null}

      <button
        type="button"
        onClick={() => {
          onToggle(!root.enabled);
        }}
        aria-pressed={root.enabled}
        aria-label={root.enabled ? `Disable ${root.name}` : `Enable ${root.name}`}
        className={cn(
          "grid h-7 w-7 shrink-0 place-items-center rounded-lg transition-colors",
          root.enabled
            ? "text-positive hover:bg-positive/10"
            : "text-white/25 hover:bg-white/[0.06] hover:text-white/60",
        )}
      >
        <Power className="h-3.5 w-3.5" aria-hidden="true" />
      </button>

      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${root.name}`}
        className="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-white/25 transition-colors hover:bg-critical/10 hover:text-critical"
      >
        <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

function ScanSection(): JSX.Element {
  const scan = useLibraryStore((state) => state.scan);
  const progress = useLibraryStore((state) => state.progress);
  const startScan = useLibraryStore((state) => state.startScan);
  const cancelScan = useLibraryStore((state) => state.cancelScan);
  const loadRuns = libraryApi.scanRuns;

  const running = scan?.running === true || progress !== null;
  const last = scan?.lastRun ?? null;
  const [recentRuns, setRecentRuns] = useState<Awaited<ReturnType<typeof loadRuns>>>([]);

  useEffect(() => {
    void loadRuns(5)
      .then(setRecentRuns)
      .catch(() => {
        setRecentRuns([]);
      });
  }, [loadRuns, last?.id]);

  return (
    <Section
      title="Scanning"
      description="A normal scan is incremental: only files whose size or modification time changed are re-read. A full scan re-reads every tag."
    >
      <div className="flex flex-wrap items-center gap-2.5">
        <Button
          variant="secondary"
          icon={running ? Loader2 : RefreshCw}
          loading={running}
          onClick={() => {
            void startScan();
          }}
          disabled={running}
        >
          {running ? "Scanning…" : "Scan now"}
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            void startScan({ full: true });
          }}
          disabled={running}
        >
          Full rescan
        </Button>
        {running ? (
          <Button
            variant="ghost"
            onClick={() => {
              void cancelScan();
            }}
          >
            Cancel
          </Button>
        ) : null}
      </div>

      {progress !== null ? (
        <div className="mt-3 rounded-xl border border-white/[0.07] bg-white/[0.02] px-3.5 py-2.5">
          <div className="flex items-center justify-between text-[11.5px] text-white/55">
            <span className="capitalize">{progress.phase}</span>
            <span className="tabular">
              {formatCount(progress.filesIndexed)} indexed · {formatCount(progress.filesSkipped)}{" "}
              unchanged
              {progress.filesFailed > 0 ? ` · ${formatCount(progress.filesFailed)} failed` : ""}
            </span>
          </div>
          <div className="mt-2 h-1 overflow-hidden rounded-full bg-white/10">
            <div
              className="accent-bg h-full transition-[width] duration-300"
              style={{ width: `${String(Math.round((progress.percent ?? 0) * 100))}%` }}
            />
          </div>
          {progress.currentPath !== null ? (
            <p className="mt-1.5 truncate font-mono text-[10px] text-white/20">
              {progress.currentPath}
            </p>
          ) : null}
        </div>
      ) : null}

      {recentRuns.length > 0 ? (
        <div className="mt-3 overflow-hidden rounded-xl border border-white/[0.06]">
          <table className="w-full text-left text-[11.5px]">
            <thead className="bg-white/[0.03] text-[10px] tracking-[0.12em] text-white/30 uppercase">
              <tr>
                <th className="px-3 py-1.5 font-medium">Started</th>
                <th className="px-3 py-1.5 font-medium">Status</th>
                <th className="px-3 py-1.5 text-right font-medium">Indexed</th>
                <th className="px-3 py-1.5 text-right font-medium">Skipped</th>
                <th className="px-3 py-1.5 text-right font-medium">Failed</th>
                <th className="px-3 py-1.5 text-right font-medium">Duration</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/[0.05]">
              {recentRuns.map((run) => (
                <tr key={run.id}>
                  <td className="px-3 py-1.5 text-white/50">
                    {new Date(run.startedAt).toLocaleString("en-GB", {
                      day: "2-digit",
                      month: "short",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </td>
                  <td className="px-3 py-1.5">
                    <span
                      className={cn(
                        "inline-flex items-center gap-1",
                        run.status === "completed"
                          ? "text-positive"
                          : run.status === "failed"
                            ? "text-critical"
                            : "text-warning",
                      )}
                    >
                      {run.status === "completed" ? (
                        <CheckCircle2 className="h-3 w-3" aria-hidden="true" />
                      ) : (
                        <AlertTriangle className="h-3 w-3" aria-hidden="true" />
                      )}
                      {run.status}
                    </span>
                  </td>
                  <td className="tabular px-3 py-1.5 text-right text-white/60">
                    {formatCount(run.filesIndexed)}
                  </td>
                  <td className="tabular px-3 py-1.5 text-right text-white/40">
                    {formatCount(run.filesSkipped)}
                  </td>
                  <td className="tabular px-3 py-1.5 text-right text-white/40">
                    {formatCount(run.filesFailed)}
                  </td>
                  <td className="tabular px-3 py-1.5 text-right text-white/40">
                    {run.durationMs === null ? "—" : `${(run.durationMs / 1000).toFixed(1)} s`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}

      {last !== null && last.errorMessage !== null ? (
        <p role="alert" className="mt-2 text-[11.5px] text-critical">
          {last.errorMessage}
        </p>
      ) : null}
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Presence
// ---------------------------------------------------------------------------

function PresenceSection(): JSX.Element {
  const [status, setStatus] = useState<DiscordStatus | null>(null);
  const [clientId, setClientId] = useState("");
  const [artworkPath, setArtworkPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void discordApi
      .status()
      .then((next) => {
        setStatus(next);
        setClientId(next.clientId);
      })
      .catch(() => {
        setStatus(null);
      });
    void discordApi
      .artworkOverridesPath()
      .then(setArtworkPath)
      .catch(() => {
        setArtworkPath(null);
      });
  }, []);

  const apply = async (
    enabled: boolean,
    showPaused = status?.showPaused ?? true,
  ): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const next = await discordApi.configure({
        enabled,
        clientId: clientId.trim(),
        showCover: status?.showCover ?? true,
        showTimestamps: status?.showTimestamps ?? true,
        showQuality: status?.showQuality ?? true,
        showPaused,
      });
      setStatus(next);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Presence could not be configured.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      title="Discord presence"
      description="Publish the track you are listening to. Prism needs its own Discord application id; nothing is sent anywhere else."
    >
      <Row label="Application id" hint="From the Discord Developer Portal. 17–20 digits.">
        <input
          type="text"
          inputMode="numeric"
          value={clientId}
          onChange={(event) => {
            setClientId(event.target.value.replace(/\D/gu, ""));
          }}
          placeholder="123456789012345678"
          aria-label="Discord application id"
          className="h-8 w-[240px] rounded-lg border border-white/10 bg-white/[0.04] px-2.5 font-mono text-[12px] text-white/85 placeholder:text-white/20"
        />
      </Row>

      <Row label="State" hint="Prism reconnects automatically if Discord is closed or restarted.">
        <div className="flex items-center gap-2">
          <StatusPill
            tone={
              status?.state === "connected"
                ? "positive"
                : status?.state === "reconnecting"
                  ? "warning"
                  : status?.state === "unavailable"
                    ? "critical"
                    : "neutral"
            }
            pulse={status?.state === "connected"}
          >
            {status === null ? "Unavailable" : status.state}
          </StatusPill>
          {status?.lastError !== null && status?.lastError !== undefined ? (
            <span
              className="max-w-[280px] truncate text-[11px] text-white/30"
              title={status.lastError}
            >
              {status.lastError}
            </span>
          ) : null}
        </div>
      </Row>

      <Row
        label="While paused"
        hint="Keep the current song visible, or clear Discord until you resume."
      >
        <Segmented
          label="Discord activity while paused"
          layoutId="settings-discord-paused"
          value={(status?.showPaused ?? true) ? "show" : "hide"}
          onChange={(next) => {
            void apply(status?.enabled ?? false, next === "show");
          }}
          options={[
            { value: "show", label: "Show activity" },
            { value: "hide", label: "Hide activity" },
          ]}
        />
      </Row>

      <Row
        label="Manual artwork URLs"
        hint="Albums without a public image are added with url: null. Paste an HTTPS URL and save the JSON file; Prism reloads it automatically."
      >
        <div className="flex max-w-[420px] flex-col items-end gap-1.5">
          <Button
            variant="ghost"
            icon={FolderOpen}
            onClick={() => {
              void discordApi.openArtworkFolder().catch((cause: unknown) => {
                setError(
                  cause instanceof Error ? cause.message : "The folder could not be opened.",
                );
              });
            }}
          >
            Open artwork folder
          </Button>
          {artworkPath !== null ? (
            <span
              className="max-w-full truncate font-mono text-[10px] text-white/25"
              title={artworkPath}
            >
              {artworkPath}
            </span>
          ) : null}
        </div>
      </Row>

      <div className="flex items-center gap-2">
        <Button
          variant="secondary"
          loading={busy}
          onClick={() => {
            void apply(true);
          }}
          disabled={clientId.trim().length === 0}
        >
          Enable presence
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            void apply(false);
          }}
          disabled={busy}
        >
          Disable
        </Button>
      </div>

      {error !== null ? (
        <p role="alert" className="text-[11.5px] text-critical">
          {error}
        </p>
      ) : null}
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

function AppearanceSection(): JSX.Element {
  const accentSource = useUiStore((state) => state.accentSource);
  const accentColor = useUiStore((state) => state.accentColor);
  const glassOpacity = useUiStore((state) => state.glassOpacity);
  const tintStrength = useUiStore((state) => state.tintStrength);
  const corners = useUiStore((state) => state.corners);
  const density = useUiStore((state) => state.density);
  const coverSize = useUiStore((state) => state.coverSize);
  const intensity = useUiStore((state) => state.ambientIntensity);
  const showRailArrows = useUiStore((state) => state.showRailArrows);
  const showHomeStats = useUiStore((state) => state.showHomeStats);
  const showSpectrum = useUiStore((state) => state.showSpectrum);
  const setAppearance = useUiStore((state) => state.setAppearance);
  const effects = useUiStore((state) => state.effects);
  const measured = useUiStore((state) => state.measuredEffects);
  const reset = useUiStore((state) => state.resetAppearance);

  return (
    <Section
      title="Appearance"
      description="Adjust how Prism looks. Every choice is applied immediately and remembered on this computer."
    >
      <Row label="Accent" hint="Follow the playing artwork, or pin a colour of your own.">
        <div className="flex items-center gap-2">
          <Segmented
            label="Accent source"
            layoutId="settings-accent-source"
            value={accentSource}
            onChange={(next) => {
              setAppearance({ accentSource: next === "fixed" ? "fixed" : "artwork" });
            }}
            options={[
              { value: "artwork", label: "Artwork" },
              { value: "fixed", label: "Fixed" },
            ]}
          />
          <input
            type="color"
            aria-label="Accent colour"
            value={accentColor}
            disabled={accentSource !== "fixed"}
            onChange={(event) => {
              setAppearance({ accentColor: event.target.value });
            }}
            className="h-8 w-9 cursor-pointer rounded-lg border border-white/10 bg-transparent disabled:opacity-30"
          />
        </div>
      </Row>

      <Row label="Surface glass" hint="Opacity of every frosted panel over the black.">
        <Percent
          label="Surface glass"
          value={glassOpacity}
          onChange={(value) => {
            setAppearance({ glassOpacity: value });
          }}
        />
      </Row>

      <Row label="Colour bleed" hint="How far the cover palette tints the surfaces behind it.">
        <Percent
          label="Colour bleed"
          value={tintStrength}
          onChange={(value) => {
            setAppearance({ tintStrength: value });
          }}
        />
      </Row>

      <Row label="Ambient canvas" hint="The slow gradient behind the content.">
        <Segmented
          label="Ambient intensity"
          layoutId="settings-ambient"
          value={intensity}
          onChange={(next) => {
            setAppearance({ ambientIntensity: next });
          }}
          options={[
            { value: "off", label: "Off" },
            { value: "subtle", label: "Subtle" },
            { value: "full", label: "Full" },
          ]}
        />
      </Row>

      <Row
        label="Visual effects"
        hint={
          effects === "auto"
            ? `Prism chooses the best balance for this computer${measured === "reduced" ? ". Simpler effects are currently active." : "."}`
            : effects === "reduced"
              ? "A simpler, faster presentation with fewer visual layers."
              : "All visual layers, including frosted panels and moving background colour."
        }
      >
        <Segmented
          label="Visual effects"
          layoutId="settings-effects"
          value={effects}
          onChange={(next) => {
            setAppearance({
              effects: next === "full" ? "full" : next === "reduced" ? "reduced" : "auto",
            });
          }}
          options={[
            { value: "auto", label: "Auto" },
            { value: "full", label: "Full" },
            { value: "reduced", label: "Reduced" },
          ]}
        />
      </Row>

      <Row label="Corners" hint="Radius of covers, cards and panels.">
        <Segmented
          label="Corner style"
          layoutId="settings-corners"
          value={corners}
          onChange={(next) => {
            setAppearance({ corners: next });
          }}
          options={[
            { value: "sharp", label: "Sharp" },
            { value: "soft", label: "Soft" },
            { value: "round", label: "Round" },
          ]}
        />
      </Row>

      <Row label="Density" hint="Vertical rhythm of lists and grids.">
        <Segmented
          label="Density"
          layoutId="settings-density"
          value={density}
          onChange={(next) => {
            setAppearance({ density: next });
          }}
          options={[
            { value: "comfortable", label: "Comfortable" },
            { value: "compact", label: "Compact" },
          ]}
        />
      </Row>

      <Row
        label="Cover size"
        hint="Edge length of the library mosaic. The grid reflows to as many columns as fit."
      >
        <div className="flex items-center gap-2.5">
          <input
            type="range"
            min={COVER_SIZE_MIN}
            max={COVER_SIZE_MAX}
            step={4}
            value={coverSize}
            aria-label="Cover size"
            onChange={(event) => {
              setAppearance({ coverSize: Number(event.target.value) });
            }}
            className="h-1 w-40 cursor-pointer appearance-none rounded-full bg-white/15 [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white"
          />
          <span className="tabular w-9 text-right text-[11px] text-white/40">{coverSize}</span>
        </div>
      </Row>

      <Row label="Rail arrows" hint="Show the corner buttons on every Home shelf.">
        <Toggle
          label="Rail arrows"
          checked={showRailArrows}
          onChange={(checked) => {
            setAppearance({ showRailArrows: checked });
          }}
        />
      </Row>

      <Row label="Home counters" hint="The listening summary under the shelves.">
        <Toggle
          label="Home counters"
          checked={showHomeStats}
          onChange={(checked) => {
            setAppearance({ showHomeStats: checked });
          }}
        />
      </Row>

      <Row label="Spectrum" hint="Show the live frequency analyser in Now Playing.">
        <Toggle
          label="Spectrum"
          checked={showSpectrum}
          onChange={(checked) => {
            setAppearance({ showSpectrum: checked });
          }}
        />
      </Row>

      <div className="pt-1">
        <Button variant="ghost" onClick={reset}>
          Reset appearance
        </Button>
      </div>
    </Section>
  );
}

function Percent({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}): JSX.Element {
  return (
    <div className="flex items-center gap-2.5">
      <input
        type="range"
        min={0}
        max={100}
        step={1}
        value={value}
        aria-label={label}
        onChange={(event) => {
          onChange(Number(event.target.value));
        }}
        className="h-1 w-40 cursor-pointer appearance-none rounded-full bg-white/15 [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white"
      />
      <span className="tabular w-9 text-right text-[11px] text-white/40">{value}</span>
    </div>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}): JSX.Element {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => {
        onChange(!checked);
      }}
      className={cn(
        "relative h-5 w-9 rounded-full border transition-colors",
        checked ? "border-accent/50 bg-accent/35" : "border-white/15 bg-white/[0.06]",
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          "absolute top-1/2 h-3.5 w-3.5 -translate-y-1/2 rounded-full bg-white transition-[left]",
          checked ? "left-[18px]" : "left-[3px]",
        )}
      />
    </button>
  );
}

// ---------------------------------------------------------------------------
// Layout primitives
// ---------------------------------------------------------------------------

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}): JSX.Element {
  return (
    <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-5">
      <h2 className="text-[15px] font-semibold tracking-[-0.01em] text-white">{title}</h2>
      <p className="mt-1 mb-4 max-w-2xl text-[12px] leading-relaxed text-white/35">{description}</p>
      <div className="space-y-3.5">{children}</div>
    </section>
  );
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint: string;
  children: ReactNode;
}): JSX.Element {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0">
        <p className="text-[12.5px] text-white/75">{label}</p>
        <p className="text-[11px] text-white/25">{hint}</p>
      </div>
      {children}
    </div>
  );
}
