/** Time-based equivalents of effects originally tuned at fixed frame rates. */
export function exponentialDecay(
  reference: number,
  elapsedMs: number,
  referenceMs: number,
): number {
  return Math.pow(reference, elapsedMs / referenceMs);
}

export function decayingDistance(
  velocityPerMs: number,
  decay: number,
  referenceDecay: number,
  referenceMs: number,
): number {
  return (velocityPerMs * referenceMs * (1 - decay)) / (1 - referenceDecay);
}
