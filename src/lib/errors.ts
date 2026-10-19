import { IpcFailure } from "@/lib/ipc";

/** Normalised error shape consumed by error boundaries and banners. */
export interface UiError {
  code: string;
  message: string;
}

/**
 * Convert anything thrown by the IPC layer or React into a renderable error.
 *
 * Never returns an empty message: a failure without information is worse than
 * a generic fallback, and the code lets the UI choose remediation guidance.
 */
export function toUiError(error: unknown): UiError {
  if (error instanceof IpcFailure) {
    return { code: error.code, message: error.message };
  }
  if (error instanceof Error) {
    return {
      code: "unexpected",
      message: error.message.length > 0 ? error.message : "An unexpected error occurred.",
    };
  }
  return { code: "unexpected", message: "An unknown error occurred." };
}
