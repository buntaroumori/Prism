import { invoke } from "@tauri-apps/api/core";
import { ZodError, type ZodType } from "zod";

import {
  AppInfoSchema,
  LibraryStatsSchema,
  PrismErrorPayloadSchema,
  RuntimeHealthSchema,
  type AppInfo,
  type LibraryStats,
  type PrismErrorPayload,
  type RuntimeHealth,
} from "@/types/ipc";

/**
 * Structured failure raised by any backend call.
 *
 * `code` is a stable machine token (`database`, `io`, `invalid_input`, ...)
 * produced by the Rust error type; `message` is safe to render verbatim.
 */
export class IpcFailure extends Error {
  readonly code: string;

  constructor(payload: PrismErrorPayload) {
    super(payload.message);
    this.name = "IpcFailure";
    this.code = payload.code;
  }
}

function toPayload(error: unknown): PrismErrorPayload {
  const structured = PrismErrorPayloadSchema.safeParse(error);
  if (structured.success) {
    return structured.data;
  }
  if (error instanceof Error) {
    return { code: "transport", message: error.message };
  }
  return {
    code: "transport",
    message: "The desktop backend did not return a structured error.",
  };
}

/**
 * Invoke a command and validate its payload.
 *
 * Two distinct failure classes are surfaced:
 *   * `transport` / backend codes — the command ran and failed;
 *   * `contract` — the command succeeded but the payload no longer matches the
 *     TypeScript schema (deploy skew), which is a bug that must be visible.
 */
export async function command<T>(
  name: string,
  schema: ZodType<T>,
  args?: Record<string, unknown>,
): Promise<T> {
  let raw: unknown;
  try {
    raw = await invoke<unknown>(name, args);
  } catch (error) {
    throw new IpcFailure(toPayload(error));
  }

  try {
    return schema.parse(raw);
  } catch (error) {
    if (error instanceof ZodError) {
      const paths = error.issues
        .map((issue) => (issue.path.length > 0 ? issue.path.join(".") : "<root>"))
        .join(", ");
      throw new IpcFailure({
        code: "contract",
        message: `Unexpected payload from '${name}' (invalid: ${paths}).`,
      });
    }
    throw error;
  }
}

/** Invoke a command that returns unit (`null` on the wire). */
export async function commandVoid(name: string): Promise<void> {
  try {
    await invoke(name);
  } catch (error) {
    throw new IpcFailure(toPayload(error));
  }
}

/** Typed façade over the backend command surface. */
export const systemApi = {
  getAppInfo: (): Promise<AppInfo> => command("get_app_info", AppInfoSchema),
  getRuntimeHealth: (): Promise<RuntimeHealth> =>
    command("get_runtime_health", RuntimeHealthSchema),
  getLibraryStats: (): Promise<LibraryStats> => command("get_library_stats", LibraryStatsSchema),
  openLogFolder: (): Promise<void> => commandVoid("open_log_folder"),
  openDataFolder: (): Promise<void> => commandVoid("open_data_folder"),
};
