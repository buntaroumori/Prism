#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import ts from "typescript";

const source = await readFile(new URL("../src/lib/frameTiming.ts", import.meta.url), "utf8");
const code = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext },
}).outputText;
const { exponentialDecay, decayingDistance } = await import(
  `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`
);

function simulate(fps) {
  const dt = 1000 / fps;
  let velocity = 1;
  let distance = 0;
  let remaining = 1;
  for (let i = 0; i < fps; i += 1) {
    const decay = exponentialDecay(0.92, dt, 16);
    distance += decayingDistance(velocity, decay, 0.92, 16);
    velocity *= decay;
    remaining *= exponentialDecay(0.78, dt, 1000 / 60);
  }
  return { distance, velocity, remaining };
}
const baseline = simulate(60);
for (const fps of [120, 144, 240]) {
  const result = simulate(fps);
  for (const key of ["distance", "velocity", "remaining"]) {
    assert.ok(Math.abs(result[key] - baseline[key]) < 1e-8, `${key} differs at ${fps} Hz`);
  }
}
console.log("Frame timing: equivalent at 60, 120, 144 and 240 Hz");
