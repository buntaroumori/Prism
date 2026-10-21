import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState, type JSX, type ReactNode } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  FolderPlus,
  FolderX,
  Loader2,
  Power,
  RefreshCw,
  Trash2,
} from "lucide-react";

import { PluginsSection } from "@/components/settings/PluginsSection";
import { Button } from "@/components/ui/Button";
import { Segmented } from "@/components/ui/Segmented";
import { StatusPill } from "@/components/ui/StatusPill";
import { formatAudioFormat } from "@/lib/audio";
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { discordApi, libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { usePlaybackStore } from "@/stores/playbackStore";
import { useOutputStore, BUFFER_CHOICES } from "@/stores/outputStore";
import { COVER_SIZE_MAX, COVER_SIZE_MIN, useUiStore } from "@/stores/uiStore";
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
          Changes apply to the running engine immediately.
        </p>
      </header>

      <OutputSection />
      <LibrarySection />
      <ScanSection />
      <PresenceSection />
      <PluginsSection />
      <AppearanceSection />
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
      description="Prism plays through the system mixer, so it coexists with every other application and the endpoint never disappears from under one of them. The engine reads the device's live configuration at open time and matches it exactly; the rate shown below is the one the driver reports, not an assumption."
    >
      <Row
        label="Output device"
        hint="Physical playback endpoints only. Loopback and capture nodes are filtered out."
      >
        <div className="flex min-w-0 items-center gap-2">
          <select
            aria-label="Output device"
            value={deviceId ?? ""}
            onChange={(event) => {
              void setDevice(event.target.value.length === 0 ? null : event.target.value);
            }}
            className="h-8 max-w-[320px] min-w-0 truncate rounded-lg border border-white/10 bg-oled-850 px-2 text-[12px] text-white/80"
          >
            <option value="">System default</option>
            {devices.map((device) => (
              <option key={device.id} value={device.id}>
                {device.name}
              </option>
            ))}
          </select>
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
        hint="Queried from the driver at runtime. In shared mode the operating system owns this format; the engine converts to it."
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
          ? "At this size the engine keeps a handful of milliseconds in flight: seeking feels immediate, but a busy machine or a USB DAC with a loose clock will produce the occasional click. Choose it when the machine is idle and the endpoint is solid."
          : bufferFrames <= 512
            ? "The balanced setting: low enough that transport buttons feel instant, deep enough to absorb a background compile or a browser tab waking up. This is the right default for most libraries."
            : "A deep buffer trades latency for resilience: nothing can starve the device, so dropouts disappear on slow disks, network shares and virtual machines. Seeking and pausing will feel a fraction of a second behind the click."}
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
      description="Prism indexes these directories recursively. Removing a folder also removes the tracks it contributed from the catalogue — files on disk are untouched, but the library stops listing what is no longer indexed."
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
  }, []);

  const apply = async (enabled: boolean): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const next = await discordApi.configure({
        enabled,
        clientId: clientId.trim(),
        showCover: status?.showCover ?? true,
        showTimestamps: status?.showTimestamps ?? true,
        showQuality: status?.showQuality ?? true,
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

      <Row label="State" hint="The worker reconnects with exponential backoff.">
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
  const reset = useUiStore((state) => state.resetAppearance);

  return (
    <Section
      title="Appearance"
      description="Every control here writes a CSS custom property on the shell root, so the window restyles in one frame and a slider move costs no React render. Preferences persist per machine."
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

      <Row label="Spectrum" hint="The analyser in Now Playing. Off saves the FFT work entirely.">
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
