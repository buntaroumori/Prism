import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

import { toUiError, type UiError } from "@/lib/errors";
import { playerApi } from "@/lib/ipc";
import type { AudioFormat, OutputDevice, OutputMode } from "@/types/ipc";

/** Buffer sizes exposed in the UI; the engine accepts 32–8192 frames. */
export const BUFFER_CHOICES = [64, 128, 256, 512, 1024, 2048] as const;

interface OutputStore {
  /**
   * Output mode. Fixed: the product standard is shared mode, so Prism coexists
   * with every other application and never takes the endpoint hostage. The
   * field stays because `player_set_output` speaks in modes.
   */
  mode: OutputMode;
  deviceId: string | null;
  bufferFrames: number;
  devices: OutputDevice[];
  /** Endpoint configuration reported by the driver, for the selected device. */
  deviceFormat: AudioFormat | null;
  loading: boolean;
  error: UiError | null;

  loadDevices: () => Promise<void>;
  loadDeviceFormat: () => Promise<void>;
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
      mode: "shared",
      deviceId: null,
      bufferFrames: 512,
      devices: [],
      deviceFormat: null,
      loading: false,
      error: null,

      loadDevices: async () => {
        set({ loading: true });
        try {
          const devices = await playerApi.devices(get().mode);
          set({ devices, error: null });
          await get().loadDeviceFormat();
        } catch (cause) {
          set({ error: toUiError(cause) });
        } finally {
          set({ loading: false });
        }
      },

      loadDeviceFormat: async () => {
        try {
          const deviceFormat = await playerApi.deviceFormat(get().mode, get().deviceId);
          set({ deviceFormat });
        } catch {
          // A backend that cannot answer (exclusive endpoints, no driver) is
          // not an error the user can act on: the field simply stays empty and
          // the player bar falls back to the negotiated format.
          set({ deviceFormat: null });
        }
      },

      setDevice: async (deviceId) => {
        try {
          await playerApi.setOutput(get().mode, deviceId, get().bufferFrames);
          set({ deviceId, error: null });
          await get().loadDeviceFormat();
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
      version: 2,
      storage: createJSONStorage(() => localStorage),
      // `mode` is not persisted: it is a constant of the product, and a stored
      // `"auto"` from an older build would quietly reintroduce exclusive mode.
      partialize: (state) => ({
        deviceId: state.deviceId,
        bufferFrames: state.bufferFrames,
      }),
      migrate: () => ({ deviceId: null, bufferFrames: 512 }),
    },
  ),
);
