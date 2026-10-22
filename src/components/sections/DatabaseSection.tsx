import type { JSX } from "react";
import { Database, FolderOpen, ShieldCheck } from "lucide-react";

import { SectionHeading } from "@/components/SectionHeading";
import { Button } from "@/components/ui/Button";
import { DataRow, InfoCard } from "@/components/ui/InfoCard";
import { useAsyncAction } from "@/hooks/useAsyncAction";
import { formatBytes } from "@/lib/format";
import { systemApi } from "@/lib/ipc";
import type { SystemSnapshot } from "@/types/ipc";

/** Library storage health without exposing implementation jargon. */
export function DatabaseSection({ snapshot }: { snapshot: SystemSnapshot }): JSX.Element {
  const { appInfo, health } = snapshot;
  const database = health.database;
  const dataAction = useAsyncAction();
  const healthy = database.integrity === "ok";

  return (
    <section id="database" aria-labelledby="database-heading">
      <SectionHeading
        eyebrow="Storage"
        title="Library data"
        description="Prism keeps your library information and playlists together on this device."
      />
      <h2 id="database-heading" className="sr-only">
        Library storage
      </h2>

      <div className="mt-6 grid gap-4 lg:grid-cols-2">
        <InfoCard
          icon={Database}
          title="Storage health"
          description="Checked whenever you refresh this page."
        >
          <dl>
            <DataRow
              label="Condition"
              value={healthy ? "Everything looks good" : "Needs attention"}
            />
            <DataRow label="Space used" value={formatBytes(database.sizeBytes)} />
            <DataRow label="Library file" value={appInfo.databasePath} mono />
          </dl>
        </InfoCard>

        <InfoCard
          icon={FolderOpen}
          title="Data folder"
          description="Open this folder when support asks for your library data."
          action={
            <Button
              variant="ghost"
              icon={FolderOpen}
              loading={dataAction.isRunning}
              onClick={() => {
                dataAction.run(() => systemApi.openDataFolder());
              }}
              className="h-8 px-2.5 text-[12px]"
            >
              Open
            </Button>
          }
        >
          <dl>
            <DataRow label="Location" value={appInfo.dataDir} mono />
          </dl>
          {dataAction.error !== null ? (
            <p role="alert" className="mt-3 text-[12px] text-critical">
              {dataAction.error.message}
            </p>
          ) : null}
        </InfoCard>
      </div>

      <div className="glass-subtle mt-4 flex items-start gap-3 rounded-3xl p-5">
        <ShieldCheck
          className="mt-0.5 h-[18px] w-[18px] shrink-0 text-positive"
          aria-hidden="true"
        />
        <div>
          <p className="text-[13px] font-semibold text-white/85">Your files are not moved</p>
          <p className="mt-1 text-[12px] leading-relaxed text-white/40">
            Scanning and organising your library never rewrites or deletes the music in your
            folders.
          </p>
        </div>
      </div>
    </section>
  );
}
