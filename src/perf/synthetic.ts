/**
 * Synthetic catalogue for the performance harness.
 *
 * This file is imported **only** by `src/perf/benchmark.tsx`, which is not part
 * of the application bundle (`vite.bench.config.ts` builds it separately). No
 * product surface ever sees synthetic data; the fixture exists so the
 * 500 000-row requirement can be measured without shipping someone's library.
 */

import type { TrackQuery, TrackRow } from "@/types/ipc";

export const SYNTHETIC_TRACKS = 500_000;

const ARTISTS = [
  "Radiohead",
  "Björk",
  "Massive Attack",
  "Portishead",
  "Aphex Twin",
  "Boards of Canada",
  "Jon Hopkins",
  "Nils Frahm",
  "Ólafur Arnalds",
  "Max Cooper",
] as const;

const ALBUMS = [
  "Kid A",
  "Homogenic",
  "Mezzanine",
  "Dummy",
  "Selected Ambient Works",
  "Music Has the Right to Children",
  "Immunity",
  "Spaces",
  "Re:member",
  "Emergence",
] as const;

const QUALITIES = ["Lossless", "Hi-Res", "CD", "MQA"] as const;

/** Pick from a fixed table without ever yielding `undefined`. */
function pick<T>(table: readonly T[], index: number, fallback: T): T {
  return table[((index % table.length) + table.length) % table.length] ?? fallback;
}

/** FNV-1a: the same deterministic hash family the album cache uses. */
function hash(value: string): number {
  let accumulator = 0x811c_9dc5;
  for (let index = 0; index < value.length; index += 1) {
    accumulator ^= value.charCodeAt(index);
    accumulator = Math.imul(accumulator, 0x0100_0193) >>> 0;
  }
  return accumulator;
}

/** One row of the fixture, derived purely from its index. */
export function syntheticRow(index: number): TrackRow {
  const albumIndex = Math.floor(index / 12);
  const artistIndex = Math.floor(index / 48);
  const artist = pick(ARTISTS, artistIndex, "Unknown artist");
  const album = pick(ALBUMS, albumIndex, "Unknown album");
  const quality = pick(QUALITIES, index, "Lossless");
  const id = `synth-${String(index).padStart(7, "0")}`;
  const movement = (index % 12) + 1;

  return {
    id,
    path: `/music/${artist}/${album}/${id}.flac`,
    folder: `/music/${artist}/${album}`,
    filename: `${id}.flac`,
    source: index % 97 === 0 ? "cue" : "file",
    title: `${album} — Movement ${String(movement)}`,
    artistId: null,
    artist,
    albumId: null,
    album,
    albumArtist: artist,
    composer: "",
    genre: index % 3 === 0 ? "Electronic" : "Ambient",
    year: 1995 + (index % 25),
    trackNo: movement,
    discNo: 1,
    format: index % 5 === 0 ? "FLAC" : "ALAC",
    codecProfile: null,
    bitDepth: index % 4 === 0 ? 24 : 16,
    sampleRate: index % 4 === 0 ? 96_000 : 44_100,
    channels: 2,
    bitrateKbps: 900 + (index % 400),
    dsdRate: null,
    durationMs: 120_000 + (index % 240_000),
    quality,
    lossless: true,
    hasEmbeddedArt: index % 7 !== 0,
    missing: false,
    rating: index % 6,
    playCount: index % 40,
    addedAt: "2026-01-01T00:00:00.000Z",
    cueStartMs: index % 97 === 0 ? 0 : null,
    cueEndMs: index % 97 === 0 ? 240_000 : null,
    coverId: null,
    coverPath: null,
    coverUrl: null,
    highlight: null,
  };
}

export interface SyntheticPage {
  items: TrackRow[];
  total: number;
  limit: number;
  offset: number;
}

/**
 * Serve a page of the fixture.
 *
 * Deliberately asynchronous with a 1 ms delay: it models the SQLite round trip
 * the real list performs, so the benchmark measures the mounted list and not a
 * synchronous array no production path can produce.
 */
export async function syntheticPage(
  _query: TrackQuery,
  limit: number,
  offset: number,
): Promise<SyntheticPage> {
  await new Promise((resolve) => {
    window.setTimeout(resolve, 1);
  });
  const count = Math.max(0, Math.min(limit, SYNTHETIC_TRACKS - offset));
  const items = Array.from({ length: count }, (_, position) => syntheticRow(offset + position));
  return { items, total: SYNTHETIC_TRACKS, limit, offset };
}

/** Deterministic identity for a row, used by the harness to key DOM nodes. */
export function syntheticKey(index: number): string {
  return `row-${String(hash(`row-${String(index)}`))}`;
}
