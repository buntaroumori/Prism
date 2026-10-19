/**
 * Locale-aware formatting helpers.
 *
 * Every formatter is pure and allocation-light because the library grid and
 * player bar call them for thousands of cells per second while scrolling.
 * `Intl.NumberFormat` instances are cached per module load.
 */

const compactNumber = new Intl.NumberFormat("en", {
  notation: "compact",
  maximumFractionDigits: 1,
});
const integerNumber = new Intl.NumberFormat("en", { maximumFractionDigits: 0 });
const dateTime = new Intl.DateTimeFormat("en", {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/** Human byte size with one decimal for the unit that fits (e.g. `1.4 GB`). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), BYTE_UNITS.length - 1);
  const value = bytes / 1024 ** exponent;
  const unit = BYTE_UNITS[exponent];
  if (unit === undefined) {
    return "0 B";
  }
  return exponent === 0 ? `${integerNumber.format(value)} ${unit}` : `${value.toFixed(1)} ${unit}`;
}

/** Compact integer for counters (`512K`, `1.2M`). */
export function formatCount(value: number): string {
  if (!Number.isFinite(value) || value < 0) {
    return "0";
  }
  return value >= 10_000 ? compactNumber.format(value) : integerNumber.format(value);
}

/** Milliseconds → `1h 02m`, `4m 12s`, `8.4s`. */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "0s";
  }
  const seconds = ms / 1000;
  if (seconds < 60) {
    return `${seconds.toFixed(seconds < 10 ? 1 : 0)}s`;
  }
  const totalMinutes = Math.floor(seconds / 60);
  const remainderSeconds = Math.floor(seconds % 60);
  if (totalMinutes < 60) {
    return `${String(totalMinutes)}m ${String(remainderSeconds).padStart(2, "0")}s`;
  }
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${String(hours)}h ${String(minutes).padStart(2, "0")}m`;
}

/** Library duration in editorial form: `3 h 41 min`, `—` when empty. */
export function formatLibraryDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "—";
  }
  const totalMinutes = Math.round(ms / 60_000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours === 0) {
    return `${String(minutes)} min`;
  }
  return `${String(hours)} h ${String(minutes).padStart(2, "0")} min`;
}

/** Technical sample-rate label: `44.1 kHz`, `768 kHz`, `—`. */
export function formatSampleRate(hz: number | null): string {
  if (hz === null || !Number.isFinite(hz) || hz <= 0) {
    return "—";
  }
  const khz = hz / 1000;
  return `${khz % 1 === 0 ? khz.toFixed(0) : khz.toFixed(1)} kHz`;
}

/** Local wall-clock time used by the diagnostics header. */
export function formatClock(date: Date): string {
  return dateTime.format(date);
}
