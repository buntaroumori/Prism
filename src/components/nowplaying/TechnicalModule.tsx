import type { JSX } from "react";
import { AudioWaveform, Clock3, Gauge, Radio } from "lucide-react";

import { formatAudioFormat, latencyMs } from "@/lib/audio";
import { formatLatency } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { PlayerStatus } from "@/types/ipc";

const FALLBACK = "—";

/**
 * Signal-path read-out.
 *
 * Deliberately unglamorous: this is the panel a listener checks to confirm what
 * the DAC is really receiving. Every value is a verbatim engine field, never a
 * derived guess, and the closing sentence states the bit-perfect verdict in
 * words rather than relying on a badge colour.
 */
export function TechnicalModule({ status }: { status: PlayerStatus | null }): JSX.Element {
  const rate = status?.sourceFormat?.sampleRate ?? null;
  const latency = latencyMs(status?.latencyFrames ?? 0, rate);

  return (
    <div className="h-full rounded-3xl border border-white/[0.06] bg-white/[0.02] p-5 backdrop-blur-xl">
      <h3 className="text-[11px] font-semibold tracking-[0.14em] text-white/55 uppercase">
        Signal path
      </h3>

      <dl className="mt-4 grid grid-cols-1 gap-px overflow-hidden rounded-2xl border border-white/[0.07] bg-white/[0.03] sm:grid-cols-2">
        <Fact
          icon={Radio}
          label="Output device"
          value={status?.deviceName ?? FALLBACK}
          detail={status?.backend ?? FALLBACK}
        />
        <Fact
          icon={AudioWaveform}
          label="Device format"
          value={formatAudioFormat(status?.outputFormat ?? null)}
          detail="Negotiated by the supervisor"
        />
        <Fact
          icon={Gauge}
          label="Source format"
          value={formatAudioFormat(status?.sourceFormat ?? null)}
          detail={status?.dop === true ? "DSD over PCM carrier" : "Decoded natively"}
        />
        <Fact
          icon={Clock3}
          label="Pipeline"
          value={latency === null ? FALLBACK : formatLatency(latency)}
          detail={`${String(status?.underruns ?? 0)} underruns · ${String(status?.overruns ?? 0)} overruns`}
        />
      </dl>

      <p className="mt-4 text-[11px] leading-relaxed text-white/25">
        {status?.bitPerfect === true
          ? "The signal reaches the DAC without a single arithmetic stage: exclusive endpoint, exact format, unity gain, no DSP."
          : "The engine is applying processing (volume, DSP, resampling or a shared mixer). The badge above tells you exactly which."}
      </p>
    </div>
  );
}

function Fact({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Radio;
  label: string;
  value: string;
  detail: string;
}): JSX.Element {
  return (
    <div className="bg-oled-950/40 px-4 py-3.5">
      <dt className="flex items-center gap-1.5 text-[9.5px] font-semibold tracking-[0.16em] text-white/25 uppercase">
        <Icon className="h-3 w-3" aria-hidden="true" />
        {label}
      </dt>
      <dd className={cn("mt-1.5 truncate text-[12.5px] text-white/80")} title={value}>
        {value}
      </dd>
      <dd className="truncate text-[10.5px] text-white/25">{detail}</dd>
    </div>
  );
}
