import type { JSX } from "react";
import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

import { cn } from "@/lib/utils";

interface InfoCardProps {
  title: string;
  description?: string;
  icon: LucideIcon;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** Glass panel with an icon header, used across every diagnostics section. */
export function InfoCard({
  title,
  description,
  icon: Icon,
  action,
  children,
  className,
}: InfoCardProps): JSX.Element {
  return (
    <section className={cn("glass rounded-3xl p-6", className)}>
      <header className="flex items-start justify-between gap-4">
        <div className="flex items-start gap-3">
          <span className="mt-0.5 grid h-9 w-9 place-items-center rounded-2xl border border-white/10 bg-white/[0.05] text-white/70">
            <Icon className="h-[18px] w-[18px]" aria-hidden="true" />
          </span>
          <div>
            <h2 className="text-sm font-semibold tracking-tight text-white">{title}</h2>
            {description !== undefined ? (
              <p className="mt-1 max-w-md text-xs leading-relaxed text-white/45">{description}</p>
            ) : null}
          </div>
        </div>
        {action}
      </header>
      <div className="mt-5">{children}</div>
    </section>
  );
}

interface DataRowProps {
  label: string;
  value: ReactNode;
  mono?: boolean;
  className?: string;
}

/** Key/value row with an optically aligned label column. */
export function DataRow({ label, value, mono = false, className }: DataRowProps): JSX.Element {
  return (
    <div
      className={cn(
        "flex items-baseline justify-between gap-6 border-b border-white/[0.055] py-2.5 last:border-none",
        className,
      )}
    >
      <dt className="shrink-0 text-xs text-white/40">{label}</dt>
      <dd
        className={cn(
          "min-w-0 truncate text-right text-[13px] text-white/80",
          mono && "tabular font-mono text-[12px]",
        )}
        title={typeof value === "string" ? value : undefined}
      >
        {value}
      </dd>
    </div>
  );
}

interface StatTileProps {
  label: string;
  value: string;
  hint?: string;
}

/** Large numeric readout used for library counters. */
export function StatTile({ label, value, hint }: StatTileProps): JSX.Element {
  return (
    <div className="glass-subtle rounded-2xl px-5 py-4">
      <p className="text-[10px] font-medium tracking-[0.14em] text-white/35 uppercase">{label}</p>
      <p className="tabular mt-2 text-2xl font-semibold tracking-tight text-white">{value}</p>
      {hint !== undefined ? <p className="mt-1 text-[11px] text-white/35">{hint}</p> : null}
    </div>
  );
}
