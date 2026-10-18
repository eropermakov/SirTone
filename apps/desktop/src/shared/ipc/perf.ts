import { isTauri } from "@tauri-apps/api/core";
import { commands, type HardwareInfo } from "./bindings";

export type { HardwareInfo };

/** Cores, RAM, GPU (T2.4). Outside Tauri — what the browser tells. */
export const perfHardware = async (): Promise<HardwareInfo> =>
  isTauri()
    ? commands.perfHardware()
    : {
        cores: navigator.hardwareConcurrency || 4,
        ramMb: 8192,
        gpuName: null,
        vramMb: 0,
        discreteGpu: false,
      };
