import { invoke } from "@tauri-apps/api/core";

/** Application metadata returned by the `app_info` Tauri command. */
export interface AppInfo {
  name: string;
  version: string;
}

/** Asks the Rust backend for the application name and version. */
export function fetchAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

/** Formats the application metadata for display, e.g. `skarzh v0.1.0`. */
export function formatAppInfo(info: AppInfo): string {
  return `${info.name} v${info.version}`;
}
