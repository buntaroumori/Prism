import type { JSX } from "react";
import { FolderCog, Gauge, Globe2, ScrollText, ShieldCheck, Timer } from "lucide-react";

import { SectionHeading } from "@/components/SectionHeading";
import { Button } from "@/components/ui/Button";
import { DataRow, InfoCard } from "@/components/ui/InfoCard";
import { useAsyncAction } from "@/hooks/useAsyncAction";
import { formatDuration } from "@/lib/format";
import { systemApi } from "@/lib/ipc";
import type { SystemSnapshot } from "@/types/ipc";

/** Application version, status and support files in user-facing language. */
export function RuntimeSection({ snapshot }: { snapshot: SystemSnapshot }): JSX.Element {
  const { appInfo, health } = snapshot;
  const logAction = useAsyncAction();

  return (
    <section id="runtime" aria-labelledby="runtime-heading">
      <SectionHeading
        eyebrow="Application"
        title="About this installation"
        description="Version information and the files that can help when you need support."
      />
      <h2 id="runtime-heading" className="sr-only">
        Application information
      </h2>

      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <InfoCard icon={Gauge} title="Prism" description="The installation currently in use.">
          <dl>
            <DataRow label="Version" value={appInfo.version} />
            <DataRow label="Platform" value={appInfo.targetOs} />
            <DataRow label="Open for" value={formatDuration(health.uptimeMs)} />
            <DataRow
              label="Status"
              value={health.status === "ok" ? "Working normally" : "Needs attention"}
            />
          </dl>
        </InfoCard>

        <InfoCard
          icon={ScrollText}
          title="Support files"
          description="Logs can help diagnose a problem without exposing your music."
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
            <DataRow label="Location" value={appInfo.logDir} mono />
            <DataRow label="History" value="Kept for 14 days" />
          </dl>
          {logAction.error !== null ? (
            <p role="alert" className="mt-3 text-[12px] text-critical">
              {logAction.error.message}
            </p>
          ) : null}
        </InfoCard>
      </div>

      <div className="mt-4 grid gap-4 md:grid-cols-3">
        <Fact
          icon={Globe2}
          title="Your library is yours"
          body="Your audio files stay on this device. Prism only contacts outside services for artwork, lyrics or Discord when those features are used."
        />
        <Fact
          icon={ShieldCheck}
          title="Made for listening"
          body="Playback continues while you browse your albums, playlists and settings."
        />
        <Fact
          icon={Timer}
          title="Ready when you are"
          body="Your choices are remembered, so Prism opens with the layout and library view you chose."
        />
      </div>
    </section>
  );
}

function Fact({
  icon: Icon,
  title,
  body,
}: {
  icon: typeof Globe2;
  title: string;
  body: string;
}): JSX.Element {
  return (
    <div className="glass-subtle rounded-3xl p-5">
      <Icon className="h-[18px] w-[18px] text-accent-soft/80" aria-hidden="true" />
      <p className="mt-4 text-[13px] font-semibold tracking-tight text-white/85">{title}</p>
      <p className="mt-1.5 text-[12px] leading-relaxed text-white/40">{body}</p>
    </div>
  );
}
