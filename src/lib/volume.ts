/**
 * Perceptual volume taper.
 *
 * A linear decibel rail spends too much of its physical travel at levels that
 * are already very quiet. The quadratic curve keeps gentle, useful steps near
 * unity and progressively wider attenuation toward silence, while still using
 * the engine's complete -60…0 dB range with no dead zone.
 */
export function volumeDbFromPercent(percent: number): number {
  const position = Math.min(100, Math.max(0, percent)) / 100;
  return -60 * (1 - position) ** 2;
}

/** Inverse of `volumeDbFromPercent`, clamped to the engine's range. */
export function volumePercentFromDb(db: number): number {
  const attenuation = Math.min(60, Math.max(0, -db));
  return Math.round((1 - Math.sqrt(attenuation / 60)) * 100);
}
