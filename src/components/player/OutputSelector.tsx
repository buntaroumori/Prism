import { useEffect, type JSX } from "react";
import { Check, ChevronUp, Cpu, Headphones, Loader2, Radio } from "lucide-react";

import { Popover } from "@/components/ui/Popover";
import { formatAudioFormat } from "@/lib/audio";
import { formatSampleRate } from "@/lib/format";
import { cn } from "@/lib/utils";
import { BUFFER_CHOICES, useOutputStore } from "@/stores/outputStore";
import type { BackendKind, PlayerStatus } from "@/types/ipc";

const BACKEND_ICON: Record<BackendKind, typeof Radio> = {
  wasapiExclusive: Cpu,
  wasapiShared: Cpu,
  asio: Cpu,
  alsaHw: Radio,
  pipeWire: Radio,
  cpalShared: Headphones,
};

const BACKEND_LABEL: Record<BackendKind, string> = {
  wasapiExclusive: "WASAPI Exclusive",
  wasapiShared: "WASAPI Shared",
  asio: "ASIO",
  alsaHw: "ALSA Hardware",
  pipeWire: "PipeWire",
  cpalShared: "System Output",
};

/**
 * Output-device selector.
 *
 * The trigger shows what is *actually* open — the engine's endpoint name and
 * the driver stack — not what was requested. The popover lists every physical
 * playback endpoint, marks the system default and prints the format the driver
 * reports for it, because that is the number a listener compares against the
 * OS panel. There is no engine-mode switch: the product standard is shared
 * mode, and a player that can silently take the endpoint away from every other
 * application needs a better reason than a dropdown.
 */
export function OutputSelector({ status }: { status: PlayerStatus | null }): JSX.Element {
  const deviceId = useOutputStore((state) => state.deviceId);
  const bufferFrames = useOutputStore((state) => state.bufferFrames);
  const devices = useOutputStore((state) => state.devices);
  const loading = useOutputStore((state) => state.loading);
  const error = useOutputStore((state) => state.error);
  const loadDevices = useOutputStore((state) => state.loadDevices);
  const setDevice = useOutputStore((state) => state.setDevice);
  const setBufferFrames = useOutputStore((state) => state.setBufferFrames);

  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  const activeName = status?.deviceName ?? "No device";
  const activeBackend =
    status?.backend !== null && status?.backend !== undefined
      ? BACKEND_LABEL[status.backend]
      : "Idle";
  const Icon =
    status?.backend !== null && status?.backend !== undefined
      ? BACKEND_ICON[status.backend]
      : Headphones;

  return (
    <Popover
      label="Output device"
      align="end"
      side="top"
      trigger={({ open, toggle }) => (
        <button
          type="button"
          onClick={toggle}
          aria-expanded={open}
          aria-haspopup="dialog"
          className={cn(
            "no-drag flex h-9 max-w-[260px] items-center gap-2 rounded-full border border-white/[0.08] bg-white/[0.04] px-3 text-left transition-colors",
            "hover:bg-white/[0.07] focus-visible:ring-2 focus-visible:ring-accent/60 focus-visible:outline-none",
            open && "bg-white/[0.08]",
          )}
        >
          <Icon className="h-3.5 w-3.5 shrink-0 text-white/45" aria-hidden="true" />
          <span className="min-w-0">
            <span className="block truncate text-[11.5px] leading-tight font-medium text-white/80">
              {activeName}
            </span>
            <span className="block truncate text-[9.5px] leading-tight tracking-[0.08em] text-white/30 uppercase">
              {activeBackend}
            </span>
          </span>
          <ChevronUp
            className={cn(
              "h-3.5 w-3.5 shrink-0 text-white/30 transition-transform",
              open && "rotate-180",
            )}
            aria-hidden="true"
          />
        </button>
      )}
    >
      <div className="space-y-3 p-2">
        <div>
          <div className="mb-1 flex items-center justify-between px-1">
            <p className="text-[10px] font-semibold tracking-[0.16em] text-white/30 uppercase">
              Endpoint
            </p>
            {loading ? (
              <Loader2 className="h-3 w-3 animate-spin text-white/30" aria-hidden="true" />
            ) : null}
          </div>

          <div className="max-h-[220px] overflow-y-auto">
            <button
              type="button"
              onClick={() => {
                void setDevice(null);
              }}
              className={cn(
                "flex w-full items-center gap-2 rounded-xl px-2 py-1.5 text-left text-[12px] transition-colors",
                deviceId === null
                  ? "bg-white/[0.08] text-white"
                  : "text-white/60 hover:bg-white/[0.05]",
              )}
            >
              <Radio className="h-3.5 w-3.5 shrink-0 opacity-60" aria-hidden="true" />
              <span className="flex-1 truncate">Follow system default</span>
              {deviceId === null ? <Check className="h-3.5 w-3.5" aria-hidden="true" /> : null}
            </button>

            {devices.map((device) => {
              const selected = deviceId === device.id;
              return (
                <button
                  key={device.id}
                  type="button"
                  onClick={() => {
                    void setDevice(device.id);
                  }}
                  className={cn(
                    "flex w-full items-start gap-2 rounded-xl px-2 py-1.5 text-left text-[12px] transition-colors",
                    selected ? "bg-white/[0.08] text-white" : "text-white/60 hover:bg-white/[0.05]",
                  )}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{device.name}</span>
                    <span className="block truncate text-[10px] text-white/30">
                      {BACKEND_LABEL[device.kind]} ·{" "}
                      {device.sampleRates.length > 0
                        ? `${formatSampleRate(device.sampleRates[0] ?? null)}–${formatSampleRate(
                            device.sampleRates[device.sampleRates.length - 1] ?? null,
                          )}`
                        : "rates unknown"}
                      {device.isDefault ? " · default" : ""}
                    </span>
                  </span>
                  {selected ? (
                    <Check className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                  ) : null}
                </button>
              );
            })}

            {!loading && devices.length === 0 ? (
              <p className="px-2 py-2 text-[11px] text-white/35">
                No endpoint is reachable in this mode.
              </p>
            ) : null}
          </div>
        </div>

        <div>
          <p className="mb-1.5 px-1 text-[10px] font-semibold tracking-[0.16em] text-white/30 uppercase">
            Buffer
          </p>
          <div className="flex flex-wrap gap-1 px-1">
            {BUFFER_CHOICES.map((frames) => (
              <button
                key={frames}
                type="button"
                onClick={() => {
                  void setBufferFrames(frames);
                }}
                className={cn(
                  "tabular rounded-lg border px-2 py-1 text-[11px] transition-colors",
                  bufferFrames === frames
                    ? "accent-border bg-white/[0.10] text-white"
                    : "border-white/[0.07] text-white/50 hover:bg-white/[0.05]",
                )}
              >
                {frames}
              </button>
            ))}
          </div>
          <p className="mt-1.5 px-1 text-[10px] leading-relaxed text-white/25">
            Frames per device period. Smaller is tighter; larger survives a busy machine.
            {status?.deviceFormat === null || status?.deviceFormat === undefined
              ? ""
              : ` The system mixes at ${formatAudioFormat(status.deviceFormat)}; the endpoint's own depth is set in the OS sound panel.`}
          </p>
        </div>

        {error !== null ? (
          <p
            role="alert"
            className="rounded-xl bg-critical/[0.08] px-2 py-1.5 text-[11px] text-critical"
          >
            {error.message}
          </p>
        ) : null}
      </div>
    </Popover>
  );
}
