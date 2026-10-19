import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import { toUiError, type UiError } from "@/lib/errors";
import { playerApi } from "@/lib/ipc";
import type { OutputDevice, OutputMode } from "@/types/ipc";

/** Buffer sizes exposed in the UI; the engine accepts 32–8192 frames. */
export const BUFFER_CHOICES = [64, 128, 256, 512, 1024, 2048] as const;

interface OutputStore {
  mode: OutputMode;
  deviceId: string | null;
  bufferFrames: number;
  devices: OutputDevice[];
  loading: boolean;
  error: UiError | null;

  loadDevices: () => Promise<void>;
  setMode: (mode: OutputMode) => Promise<void>;
  setDevice: (deviceId: string | null) => Promise<void>;
  setBufferFrames: (frames: number) => Promise<void>;
  clearError: () => void;
}

/**
 * Output configuration.
 *
 * `mode`, `deviceId` and `bufferFrames` are persisted so the DAC a listener
 * chose survives a restart; the device list is always re-queried because
 * endpoints come and go. Applying a change re-opens the device at the current
 * position — the engine guarantees continuity, so there is no confirmation step.
 */
export const useOutputStore = create<OutputStore>()(
  persist(
    (set, get) => ({
      mode: "auto",
      deviceId: null,
      bufferFrames: 512,
      devices: [],
      loading: false,
      error: null,

      loadDevices: async () => {
        set({ loading: true });
        try {
          const devices = await playerApi.devices(get().mode);
          set({ devices, error: null });
        } catch (cause) {
          set({ error: toUiError(cause) });
        } finally {
          set({ loading: false });
        }
      },

      setMode: async (mode) => {
        try {
          await playerApi.setOutput(mode, get().deviceId, get().bufferFrames);
          set({ mode, error: null });
          await get().loadDevices();
        } catch (cause) {
          set({ error: toUiError(cause) });
        }
      },

      setDevice: async (deviceId) => {
        try {
          await playerApi.setOutput(get().mode, deviceId, get().bufferFrames);
          set({ deviceId, error: null });
        } catch (cause) {
          set({ error: toUiError(cause) });
        }
      },

      setBufferFrames: async (bufferFrames) => {
        try {
          await playerApi.setOutput(get().mode, get().deviceId, bufferFrames);
          set({ bufferFrames, error: null });
        } catch (cause) {
          set({ error: toUiError(cause) });
        }
      },

      clearError: () => {
        set({ error: null });
      },
    }),
    {
      name: "prism.output.preferences",
      version: 1,
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        mode: state.mode,
        deviceId: state.deviceId,
        bufferFrames: state.bufferFrames,
      }),
    },
  ),
);
