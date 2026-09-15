import { useEffect } from "react";
import type { Settings } from "../lib/types";
export function useTheme(settings: Settings) {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark =
        settings.theme === "dark" ||
        (settings.theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
      document.documentElement.dataset.compact = String(settings.compact);
      document.documentElement.dataset.reducedMotion = String(
        settings.reducedMotion,
      );
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings]);
}
