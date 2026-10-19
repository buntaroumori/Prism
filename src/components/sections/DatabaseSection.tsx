import type { JSX } from "react";
import { Database, FolderOpen, ShieldCheck } from "lucide-react";

import { SectionHeading } from "@/components/SectionHeading";
import { Button } from "@/components/ui/Button";
import { DataRow, InfoCard } from "@/components/ui/InfoCard";
import { useAsyncAction } from "@/hooks/useAsyncAction";
import { formatBytes, formatCount } from "@/lib/format";
import { systemApi } from "@/lib/ipc";
import type { SystemSnapshot } from "@/types/ipc";

interface DatabaseSectionProps {
  snapshot: SystemSnapshot;
}

const SYNCHRONOUS_LABELS: Record<number, string> = {
  0: "OFF",
  1: "NORMAL",
  2: "FULL",
  3: "EXTRA",
};

/**
 * Storage diagnostics.
 *
 * Everything is read from SQLite pragmas: journal mode, durability level,
 * `quick_check(1)`, page geometry and the sqlx migration ledger. A user can
 * export this screen when filing a bug instead of guessing.
 */
export function DatabaseSection({ snapshot }: DatabaseSectionProps): JSX.Element {
  const { appInfo, health } = snapshot;
  const database = health.database;
  const dataAction = useAsyncAction();

  const synchronous = SYNCHRONOUS_LABELS[database.synchronous] ?? `Mode ${database.synchronous}`;
  const integrityOk = database.integrity === "ok";

  return (
    <section id="database" aria-labelledby="database-heading" className="scroll-mt-24">
      <SectionHeading
        eyebrow="Storage"
        title="Catalogue engine"
        description="SQLite in WAL mode with a single writer pool, embedded migrations and a bounded integrity probe."
      />
      <h2 id="database-heading" className="sr-only">
        Database health
      </h2>

      <div className="mt-6 grid gap-4 lg:grid-cols-[1.15fr_0.85fr]">
        <InfoCard
          icon={Database}
          title="SQLite runtime"
          description="Values captured with cheap pragmas at refresh time."
        >
          <dl>
            <DataRow label="Journal mode" value={database.journalMode.toUpperCase()} mono />
            <DataRow label="Synchronous" value={synchronous} mono />
            <DataRow
              label="Integrity probe"
              value={integrityOk ? "ok · quick_check" : database.integrity}
            />
            <DataRow label="Database size" value={formatBytes(database.sizeBytes)} />
            <DataRow
              label="Pages"
              value={`${formatCount(database.pageCount)} × ${formatBytes(database.pageSize)}`}
              mono
            />
            <DataRow label="Freelist" value={`${formatCount(database.freelistCount)} pages`} />
            <DataRow
              label="Schema version"
              value={`v${String(database.migrationVersion)} · ${String(database.appliedMigrations)} migration(s)`}
            />
            <DataRow
              label="Connection pool"
              value={`${String(database.poolIdle)} idle / ${String(database.poolSize)} open`}
            />
          </dl>
        </InfoCard>

        <div className="flex flex-col gap-4">
          <InfoCard
            icon={FolderOpen}
            title="Locations"
            description="Prism writes only inside its application directories."
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
              <DataRow label="Database" value={appInfo.databasePath} mono />
              <DataRow label="Data directory" value={appInfo.dataDir} mono />
              <DataRow label="Log directory" value={appInfo.logDir} mono />
            </dl>
            {dataAction.error !== null ? (
              <p role="alert" className="mt-3 text-[12px] text-critical">
                {dataAction.error.message}
              </p>
            ) : null}
          </InfoCard>

          <div className="glass-subtle rounded-3xl p-6">
            <div className="flex items-start gap-3">
              <span className="mt-0.5 grid h-9 w-9 place-items-center rounded-2xl border border-positive/20 bg-positive/[0.07] text-positive">
                <ShieldCheck className="h-[18px] w-[18px]" aria-hidden="true" />
              </span>
              <div>
                <p className="text-sm font-semibold tracking-tight text-white">Durability model</p>
                <p className="mt-1 text-[12px] leading-relaxed text-white/40">
                  WAL with <code className="font-mono text-white/55">synchronous=NORMAL</code>:
                  committed transactions survive application crashes; a kernel panic can only
                  discard the last uncheckpointed batch. The library index is resumable by design.
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
