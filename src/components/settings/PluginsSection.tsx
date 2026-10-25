import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState, type JSX, type ReactNode } from "react";
import { Cpu, FolderOpen, Puzzle, RefreshCw, ShieldCheck, TriangleAlert } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { PopoverSelect } from "@/components/ui/PopoverSelect";
import { StatusPill } from "@/components/ui/StatusPill";
import { hasBackend } from "@/lib/cover";
import { toUiError } from "@/lib/errors";
import { EVENT_CHANNELS, parsePluginMessage, pluginsApi } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import type { PluginDescriptor, PluginMessage, PluginSetting, PluginSnapshot } from "@/types/ipc";

/** Log lines kept on screen. The backend already rate-limits plugins to 40/s. */
const LOG_LIMIT = 24;

/**
 * Extensibility settings.
 *
 * Three things a plugin system needs in the UI to be trustworthy, all present
 * here: what is installed (with the exact directory), what each plugin is
 * *allowed* to do (declared capabilities, not a vague "trusted" badge), and what
 * it is actually doing right now (the live message stream). Toggling a plugin
 * re-instantiates it through the backend and replaces the snapshot with the
 * answer, so the switch always reflects real state rather than the click.
 */
export function PluginsSection(): JSX.Element {
  const [snapshot, setSnapshot] = useState<PluginSnapshot | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [messages, setMessages] = useState<PluginMessage[]>([]);

  useEffect(() => {
    if (!hasBackend()) {
      return;
    }
    let active = true;
    void pluginsApi
      .list()
      .then((next) => {
        if (active) {
          setSnapshot(next);
        }
      })
      .catch((cause: unknown) => {
        if (active) {
          setError(toUiError(cause).message);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!hasBackend()) {
      return;
    }
    let unlisten: UnlistenFn | null = null;
    let active = true;
    void listen<unknown>(EVENT_CHANNELS.pluginMessage, (event) => {
      const message = parsePluginMessage(event.payload);
      if (message === null) {
        return;
      }
      setMessages((previous) => [message, ...previous].slice(0, LOG_LIMIT));
    }).then((stop) => {
      if (active) {
        unlisten = stop;
      } else {
        stop();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const apply = async (action: string, run: () => Promise<PluginSnapshot>): Promise<void> => {
    setBusy(action);
    setError(null);
    try {
      setSnapshot(await run());
    } catch (cause) {
      setError(toUiError(cause).message);
    } finally {
      setBusy(null);
    }
  };

  const plugins = snapshot?.plugins ?? [];
  const noRuntimes = snapshot !== null && !snapshot.runtimes.lua && !snapshot.runtimes.wasm;

  return (
    <section className="rounded-2xl border border-white/[0.07] bg-white/[0.02] p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-[15px] font-semibold tracking-[-0.01em] text-white">
            <Puzzle className="h-4 w-4 text-white/45" aria-hidden="true" />
            Plugins
          </h2>
          <p className="mt-1 max-w-2xl text-[12px] leading-relaxed text-white/35">
            Sandboxed Lua and WebAssembly modules. A plugin gets only the capabilities its manifest
            declares; playback does not depend on any of them.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            icon={FolderOpen}
            className="h-8"
            onClick={() => {
              void pluginsApi.openFolder().catch((cause: unknown) => {
                setError(toUiError(cause).message);
              });
            }}
          >
            Folder
          </Button>
          <Button
            variant="secondary"
            icon={RefreshCw}
            className="h-8"
            loading={busy === "reload"}
            onClick={() => {
              void apply("reload", () => pluginsApi.reload());
            }}
          >
            Rescan
          </Button>
        </div>
      </div>

      {snapshot !== null ? (
        <div className="mt-3 flex flex-wrap items-center gap-2 text-[10.5px] text-white/30">
          <StatusPill tone={snapshot.runtimes.lua ? "positive" : "neutral"} className="h-6">
            Lua {snapshot.runtimes.lua ? "available" : "not built"}
          </StatusPill>
          <StatusPill tone={snapshot.runtimes.wasm ? "positive" : "neutral"} className="h-6">
            WASM {snapshot.runtimes.wasm ? "available" : "not built"}
          </StatusPill>
          <span className="tabular rounded-full border border-white/[0.08] px-2 py-1">
            API v{String(snapshot.runtimes.api)}
          </span>
          <span className="truncate font-mono text-[10px] text-white/20">{snapshot.directory}</span>
        </div>
      ) : null}

      {noRuntimes ? (
        <p className="mt-3 flex items-start gap-2 rounded-xl border border-warning/20 bg-warning/[0.05] px-3 py-2 text-[11.5px] leading-relaxed text-warning/80">
          <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          This build was compiled without the plugin runtimes. Manifests are still listed and
          validated; rebuild with <code className="mx-1 font-mono">--features lua-plugins</code>
          or <code className="mx-1 font-mono">--features wasm-plugins</code> to execute them.
        </p>
      ) : null}

      {error !== null ? (
        <p className="mt-3 rounded-xl border border-critical/20 bg-critical/[0.06] px-3 py-2 text-[11.5px] text-critical">
          {error}
        </p>
      ) : null}

      {plugins.length === 0 ? (
        <div className="mt-4 flex flex-col items-center gap-2 rounded-xl border border-dashed border-white/[0.08] px-6 py-8 text-center">
          <Cpu className="h-5 w-5 text-white/20" aria-hidden="true" strokeWidth={1.5} />
          <p className="text-[12.5px] text-white/50">No plugins installed</p>
          <p className="max-w-md text-[11px] leading-relaxed text-white/25">
            A plugin is a folder containing <code className="font-mono">plugin.toml</code> and an
            entry file. Drop one in the plugin folder and rescan.
          </p>
        </div>
      ) : (
        <ul className="mt-4 space-y-3">
          {plugins.map((plugin) => (
            <PluginCard
              key={plugin.id}
              plugin={plugin}
              busy={busy === plugin.id}
              onToggle={() => {
                void apply(plugin.id, () => pluginsApi.setEnabled(plugin.id, !plugin.enabled));
              }}
              onConfig={(key, value) => {
                void apply(plugin.id, () => pluginsApi.setConfig(plugin.id, key, value));
              }}
            />
          ))}
        </ul>
      )}

      {messages.length > 0 ? (
        <div className="mt-4">
          <div className="flex items-center justify-between">
            <p className="text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase">
              Plugin activity
            </p>
            <button
              type="button"
              onClick={() => {
                setMessages([]);
              }}
              className="text-[10.5px] text-white/25 hover:text-white/60"
            >
              Clear
            </button>
          </div>
          <ul className="mt-1.5 max-h-40 scrollbar-thin space-y-1 overflow-y-auto rounded-xl bg-black/20 p-2 font-mono text-[10.5px]">
            {messages.map((message, position) => (
              <li
                key={`${String(position)}-${message.pluginId}-${message.message.slice(0, 16)}`}
                className="flex gap-2"
              >
                <span className="shrink-0 text-white/20">{message.pluginId}</span>
                <span
                  className={cn(
                    "min-w-0 break-words",
                    message.level === "error"
                      ? "text-critical/80"
                      : message.level === "warn"
                        ? "text-warning/80"
                        : "text-white/45",
                  )}
                >
                  {message.message}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}

function PluginCard({
  plugin,
  busy,
  onToggle,
  onConfig,
}: {
  plugin: PluginDescriptor;
  busy: boolean;
  onToggle: () => void;
  onConfig: (key: string, value: string | number | boolean) => void;
}): JSX.Element {
  return (
    <li
      className={cn(
        "rounded-xl border p-3.5 transition-colors",
        plugin.enabled && plugin.loaded
          ? "border-white/[0.08] bg-white/[0.025]"
          : "border-white/[0.05] bg-white/[0.01]",
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <p className="truncate text-[13px] font-medium text-white/85">{plugin.name}</p>
            <span className="tabular rounded-full border border-white/[0.08] px-1.5 py-px text-[9.5px] text-white/35">
              {plugin.version}
            </span>
            <span className="rounded-full bg-white/[0.05] px-1.5 py-px text-[9.5px] tracking-[0.1em] text-white/35 uppercase">
              {plugin.runtime}
            </span>
            {plugin.loaded ? (
              <span className="flex items-center gap-1 text-[9.5px] text-positive/80">
                <ShieldCheck className="h-3 w-3" aria-hidden="true" />
                loaded
              </span>
            ) : null}
          </div>
          <p className="mt-0.5 font-mono text-[10px] text-white/20">{plugin.id}</p>
          {plugin.description.length > 0 ? (
            <p className="mt-1.5 max-w-xl text-[11.5px] leading-relaxed text-white/40">
              {plugin.description}
            </p>
          ) : null}
          {plugin.capabilities.length > 0 ? (
            <div className="mt-2 flex flex-wrap gap-1">
              {plugin.capabilities.map((capability) => (
                <span
                  key={capability}
                  className="rounded bg-accent/[0.08] px-1.5 py-0.5 font-mono text-[9.5px] text-accent-soft/80"
                >
                  {capability}
                </span>
              ))}
            </div>
          ) : null}
        </div>

        <button
          type="button"
          role="switch"
          aria-checked={plugin.enabled}
          onClick={onToggle}
          disabled={busy}
          className={cn(
            "relative h-6 w-11 shrink-0 rounded-full border transition-colors",
            plugin.enabled ? "border-accent/40 bg-accent/25" : "border-white/10 bg-white/[0.05]",
            busy && "opacity-50",
          )}
          aria-label={`${plugin.enabled ? "Disable" : "Enable"} ${plugin.name}`}
        >
          <span
            className={cn(
              "absolute top-0.5 h-4.5 w-4.5 rounded-full bg-white transition-[left] duration-150",
              plugin.enabled ? "left-[22px]" : "left-0.5",
            )}
          />
        </button>
      </div>

      {plugin.error !== null ? (
        <p className="mt-2 flex items-start gap-1.5 rounded-lg border border-critical/20 bg-critical/[0.06] px-2.5 py-1.5 text-[10.5px] leading-relaxed text-critical/85">
          <TriangleAlert className="mt-0.5 h-3 w-3 shrink-0" aria-hidden="true" />
          {plugin.error}
        </p>
      ) : null}

      {plugin.settings.length > 0 && plugin.enabled ? (
        <div className="mt-3 space-y-2 border-t border-white/[0.05] pt-3">
          {plugin.settings.map((setting) => (
            <SettingControl
              key={setting.key}
              setting={setting}
              disabled={busy}
              onChange={(value) => {
                onConfig(setting.key, value);
              }}
            />
          ))}
        </div>
      ) : null}
    </li>
  );
}

function SettingControl({
  setting,
  disabled,
  onChange,
}: {
  setting: PluginSetting;
  disabled: boolean;
  onChange: (value: string | number | boolean) => void;
}): JSX.Element {
  const [draft, setDraft] = useState<string>(String(setting.value));

  // The backend echoes the coerced value; a rejected edit reverts the field.
  useEffect(() => {
    setDraft(String(setting.value));
  }, [setting.value]);

  const commitNumber = (): void => {
    const parsed = Number(draft);
    if (!Number.isFinite(parsed) || parsed === Number(setting.value)) {
      setDraft(String(setting.value));
      return;
    }
    onChange(setting.type === "integer" ? Math.round(parsed) : parsed);
  };

  return (
    <SettingRow label={setting.label} hint={setting.help ?? undefined}>
      {setting.type === "boolean" ? (
        <button
          type="button"
          role="switch"
          aria-checked={setting.value === true}
          disabled={disabled}
          onClick={() => {
            onChange(setting.value !== true);
          }}
          className={cn(
            "relative h-6 w-11 rounded-full border transition-colors",
            setting.value === true
              ? "border-accent/40 bg-accent/25"
              : "border-white/10 bg-white/[0.05]",
          )}
          aria-label={setting.label}
        >
          <span
            className={cn(
              "absolute top-0.5 h-4.5 w-4.5 rounded-full bg-white transition-[left] duration-150",
              setting.value === true ? "left-[22px]" : "left-0.5",
            )}
          />
        </button>
      ) : setting.type === "select" ? (
        <PopoverSelect
          value={String(setting.value)}
          disabled={disabled}
          ariaLabel={setting.label}
          onChange={onChange}
          options={setting.options.map((option) => ({ value: option, label: option }))}
          buttonClassName="min-w-[140px]"
          menuClassName="min-w-[180px]"
        />
      ) : setting.type === "string" ? (
        <input
          value={draft}
          disabled={disabled}
          aria-label={setting.label}
          onChange={(event) => {
            setDraft(event.target.value);
          }}
          onBlur={() => {
            if (draft !== String(setting.value)) {
              onChange(draft);
            }
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.currentTarget.blur();
            }
          }}
          className="h-8 w-40 rounded-lg border border-white/10 bg-white/[0.03] px-2 text-[11.5px] text-white/75 focus:border-accent/40 focus:outline-none"
        />
      ) : (
        <input
          type="number"
          value={draft}
          disabled={disabled}
          aria-label={setting.label}
          min={setting.min ?? undefined}
          max={setting.max ?? undefined}
          step={setting.type === "integer" ? 1 : 0.1}
          onChange={(event) => {
            setDraft(event.target.value);
          }}
          onBlur={commitNumber}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.currentTarget.blur();
            }
          }}
          className="tabular h-8 w-24 rounded-lg border border-white/10 bg-white/[0.03] px-2 text-right text-[11.5px] text-white/75 focus:border-accent/40 focus:outline-none"
        />
      )}
    </SettingRow>
  );
}

function SettingRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string | undefined;
  children: ReactNode;
}): JSX.Element {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <div className="min-w-0">
        <p className="text-[11.5px] text-white/65">{label}</p>
        {hint !== undefined ? <p className="text-[10px] text-white/25">{hint}</p> : null}
      </div>
      {children}
    </div>
  );
}
