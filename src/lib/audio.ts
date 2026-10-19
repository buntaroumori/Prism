import type { AudioFormat, SampleFormat } from "@/types/ipc";

/**
 * Display precision of a sample format, mirroring the Rust `display_bits`.
 *
 * Floats report their mantissa width (32/64), not their storage width, because
 * that is what a listener compares against a 24-bit integer file.
 */
export function displayBits(format: SampleFormat): number {
  switch (format) {
    case "s16":
      return 16;
    case "s24":
    case "s24_in32":
      return 24;
    case "s32":
    case "f32":
      return 32;
    case "f64":
      return 64;
  }
}

/** `24-bit / 96 kHz / Stereo` for a negotiated engine format. */
export function formatAudioFormat(format: AudioFormat | null): string {
  if (format === null) {
    return "—";
  }
  const bits = `${String(displayBits(format.sampleFormat))}-bit`;
  const khz = format.sampleRate / 1000;
  const rate = `${khz % 1 === 0 ? khz.toFixed(0) : khz.toFixed(1)} kHz`;
  const channels =
    format.channels === 1
      ? "Mono"
      : format.channels === 2
        ? "Stereo"
        : `${String(format.channels)} ch`;
  return `${bits} / ${rate} / ${channels}`;
}

/**
 * Frames of latency converted to time, with an honest zero.
 *
 * A "latency" computed without a known rate is not a number: returning `null`
 * lets the caller render `—` instead of `0.0 ms`, which would read as a perfect
 * (and false) measurement.
 */
export function latencyMs(frames: number, sampleRate: number | null): number | null {
  if (sampleRate === null || sampleRate <= 0 || frames <= 0) {
    return null;
  }
  return (frames / sampleRate) * 1000;
}
