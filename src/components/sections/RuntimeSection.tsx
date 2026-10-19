import type { JSX } from "react";
import { Braces, FolderCog, Gauge, Globe2, ScrollText, Timer, type LucideIcon } from "lucide-react";

import { SectionHeading } from "@/components/SectionHeading";
import { Button } from "@/components/ui/Button";
import { DataRow, InfoCard } from "@/components/ui/InfoCard";
import { useAsyncAction } from "@/hooks/useAsyncAction";
import { formatDuration } from "@/lib/format";
import { systemApi } from "@/lib/ipc";
import type { SystemSnapshot } from "@/types/ipc";

interface RuntimeSectionProps {
  snapshot: SystemSnapshot;
}

interface Guarantee {
  icon: LucideIcon;
  title: string;
  body: string;
}

const GUARANTEES: readonly Guarantee[] = [
  {
    icon: Braces,
    title: "Typed IPC contract",
    body: "Every command payload is validated with Zod on arrival. Contract drift fails as a visible error, never as undefined in the render tree.",
  },
  {
    icon: Globe2,
    title: "Local-first by construction",
    body: "The CSP only permits the artwork and metadata endpoints the engine explicitly uses. Audio files never leave the machine.",
  },
  {
    icon: Timer,
    title: "Instrumented startup",
    body: "Database, migrations and paths are resolved before the first paint; uptime is measured from the completed bootstrap.",
  },
];

/**
 * Runtime panel: build identity, log access and the engineering guarantees
 * that already hold in this build (each backed by code, not roadmap copy).
 */
export function RuntimeSection({ snapshot }: RuntimeSectionProps): JSX.Element {
  const { appInfo, health } = snapshot;
  const logAction = useAsyncAction();

  return (
    <section id="runtime" aria-labelledby="runtime-heading" className="scroll-mt-24">
      <SectionHeading
        eyebrow="Runtime"
        title="Build & environment"
        description="The exact binary serving this window, plus one-click access to its logs."
      />
      <h2 id="runtime-heading" className="sr-only">
        Runtime information
      </h2>

      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <InfoCard
          icon={Gauge}
          title="Application"
          description="Resolved at build time and at boot."
        >
          <dl>
            <DataRow label="Product" value={`${appInfo.name} ${appInfo.version}`} />
            <DataRow label="Tauri" value={appInfo.tauriVersion} mono />
            <DataRow label="Platform" value={`${appInfo.targetOs} · ${appInfo.targetArch}`} />
            <DataRow label="Build profile" value={appInfo.buildProfile} />
            <DataRow label="Uptime" value={formatDuration(health.uptimeMs)} />
            <DataRow label="Health probe" value={health.status} />
          </dl>
        </InfoCard>

        <InfoCard
          icon={ScrollText}
          title="Diagnostics logging"
          description="Dual sink: stderr for development, daily-rotating files for support."
          action={
            <Button
              variant="ghost"
              icon={FolderCog}
              loading={logAction.isRunning}
              onClick={() => {
                logAction.run(() => systemApi.openLogFolder());
              }}
              className="h-8 px-2.5 text-[12px]"
            >
              Open logs
            </Button>
          }
        >
          <dl>
            <DataRow label="Log directory" value={appInfo.logDir} mono />
            <DataRow label="Rotation" value="Daily · 14 day retention" />
            <DataRow label="Sink" value="Non-blocking worker" />
          </dl>
          {logAction.error !== null ? (
            <p role="alert" className="mt-3 text-[12px] text-critical">
              {logAction.error.message}
            </p>
          ) : null}
        </InfoCard>
      </div>

      <div className="mt-4 grid gap-4 md:grid-cols-3">
        {GUARANTEES.map((item) => (
          <div key={item.title} className="glass-subtle rounded-3xl p-5">
            <item.icon className="h-[18px] w-[18px] text-accent-soft/80" aria-hidden="true" />
            <p className="mt-4 text-[13px] font-semibold tracking-tight text-white/85">
              {item.title}
            </p>
            <p className="mt-1.5 text-[12px] leading-relaxed text-white/40">{item.body}</p>
          </div>
        ))}
      </div>
    </section>
  );
}
