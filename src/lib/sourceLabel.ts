export type SourceTone = "hi-res" | "cd" | "neutral";

export interface SourceLabel {
  label: string;
  tone: SourceTone;
}

export interface SourceMetadata {
  format: string | null;
  sampleRate: number | null;
  bitDepth: number | null;
}

export function formatSampleRate(rate: number | null): string {
  if (rate === null) return "—";
  const khz = Math.round((rate / 1000) * 10) / 10;
  return `${Number.isInteger(khz) ? khz.toFixed(0) : khz.toFixed(1)}K`;
}

/** Player-label rules shared by every album/source presentation. */
export function sourceLabel({ format, sampleRate, bitDepth }: SourceMetadata): SourceLabel {
  const codec = (format ?? "Audio").toUpperCase();
  const rate = formatSampleRate(sampleRate);
  if (codec !== "FLAC") return { label: `${rate} · ${codec}`, tone: "neutral" };
  const hiRes = (bitDepth ?? 0) > 16 || (sampleRate ?? 0) > 48_000;
  const cd = bitDepth === 16 && sampleRate === 44_100;
  const category = hiRes ? "Hi-Res" : cd ? "CD" : "Lossless";
  return {
    label: `${bitDepth === null ? "" : `${String(bitDepth)}/`}${rate} · ${category}`,
    tone: hiRes ? "hi-res" : cd ? "cd" : "neutral",
  };
}

/** Preserve all available precision without inventing missing date parts. */
export function formatReleaseDate(releaseDate: string | null, year: number | null): string {
  const value = releaseDate?.trim();
  if (value === undefined || value.length === 0) {
    return year?.toString() ?? "Unknown";
  }
  const iso = /^(\d{4})(?:-(\d{2})(?:-(\d{2}))?)?/.exec(value);
  if (iso?.[1] === undefined) return value;
  if (iso[2] === undefined) return iso[1];
  const month = Number(iso[2]);
  const day = iso[3] === undefined ? null : Number(iso[3]);
  if (month < 1 || month > 12 || (day !== null && (day < 1 || day > 31))) return value;
  const date = new Date(Date.UTC(Number(iso[1]), month - 1, day ?? 1));
  return new Intl.DateTimeFormat("en-GB", {
    year: "numeric",
    month: "short",
    ...(day === null ? {} : { day: "numeric" }),
    timeZone: "UTC",
  }).format(date);
}
