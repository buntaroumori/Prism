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
import { formatCount, formatLibraryDuration } from "@/lib/format";
import { discordApi, libraryApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { useLibraryStore } from "@/stores/libraryStore";
import { useOutputStore, BUFFER_CHOICES } from "@/stores/outputStore";
import { useUiStore } from "@/stores/uiStore";
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
  const mode = useOutputStore((state) => state.mode);
  const bufferFrames = useOutputStore((state) => state.bufferFrames);
  const devices = useOutputStore((state) => state.devices);
  const setMode = useOutputStore((state) => state.setMode);
  const setDevice = useOutputStore((state) => state.setDevice);
  const setBufferFrames = useOutputStore((state) => state.setBufferFrames);
  const loadDevices = useOutputStore((state) => state.loadDevices);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  return (
    <Section
      title="Output"
      description="How the engine reaches your DAC. Exclusive modes bypass the operating-system mixer and enable bit-perfect playback."
    >
      <Row
        label="Engine mode"
        hint="Exclusive refuses to share the device; Auto falls back when it must."
      >
        <Segmented
          label="Engine mode"
          layoutId="settings-output-mode"
          value={mode}
          onChange={(next) => {
            void setMode(next);
          }}
          options={[
            { value: "auto", label: "Auto" },
            { value: "exclusive", label: "Exclusive" },
            { value: "shared", label: "Shared" },
            { value: "asio", label: "ASIO" },
          ]}
        />
      </Row>

      <Row label="Device" hint="Endpoints reachable through the selected mode.">
        <div className="flex items-center gap-2">
          <select
            aria-label="Output device"
            onChange={(event) => {
              void setDevice(event.target.value.length === 0 ? null : event.target.value);
            }}
            defaultValue=""
            className="h-8 max-w-[320px] rounded-lg border border-white/10 bg-oled-850 px-2 text-[12px] text-white/80"
          >
            <option value="">Follow system default</option>
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

      <Row label="Buffer" hint="Frames per device period. Lower is tighter; higher survives load.">
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
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Library roots
// ---------------------------------------------------------------------------

function LibrarySection(): JSX.Element {
  const roots = useLibraryStore((state) => state.roots);
  const addRoot = useLibraryStore((state) => state.addRoot);
  const removeRoot = useLibraryStore((state) => state.removeRoot);
  const setRootEnabled = useLibraryStore((state) => state.setRootEnabled);
  const error = useLibraryStore((state) => state.error);
  const stats = useLibraryStore((state) => state.stats);
  const [picking, setPicking] = useState(false);

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
      description="Prism indexes these directories recursively. A folder that disappears is reported, never silently emptied."
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
                void removeRoot(root.id);
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
  const intensity = useUiStore((state) => state.ambientIntensity);
  const setIntensity = useUiStore((state) => state.setAmbientIntensity);

  return (
    <Section
      title="Appearance"
      description="The ambient canvas is tinted from the current artwork. It animates on the compositor only, so even the full setting costs no main-thread time."
    >
      <Row
        label="Ambient intensity"
        hint="Turn it off on battery or on an OLED panel you want truly black."
      >
        <Segmented
          label="Ambient intensity"
          layoutId="settings-ambient"
          value={intensity}
          onChange={setIntensity}
          options={[
            { value: "off", label: "Off" },
            { value: "subtle", label: "Subtle" },
            { value: "full", label: "Full" },
          ]}
        />
      </Row>
    </Section>
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
