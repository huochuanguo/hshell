import { isTauri } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export const copyText = (text: string) =>
  isTauri() ? writeText(text) : navigator.clipboard.writeText(text);
export const pasteText = () =>
  isTauri() ? readText() : navigator.clipboard.readText();
