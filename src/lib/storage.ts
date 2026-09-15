import type { Settings } from "./types";
export const defaultSettings: Settings = {
  theme: "dark",
  compact: false,
  reducedMotion: false,
  simpleMode: false,
};
export const isSettings = (value: unknown): value is Settings => {
  if (!value || typeof value !== "object") return false;
  const s = value as Settings;
  return (
    ["dark", "light", "system"].includes(s.theme) &&
    typeof s.compact === "boolean" &&
    typeof s.reducedMotion === "boolean" &&
    (typeof s.simpleMode === "boolean" || typeof s.simpleMode === "undefined")
  );
};
