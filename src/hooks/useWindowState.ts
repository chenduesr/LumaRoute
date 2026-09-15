import { useEffect, useRef } from "react";
import { isTauri } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window";

type SavedWindow = {
  x: number;
  y: number;
  width: number;
  height: number;
  maximized: boolean;
};
const key = (simple: boolean) =>
  `myray.window.${simple ? "simple" : "full"}.v1`;

export function useWindowState(simpleMode: boolean) {
  const mode = useRef(simpleMode);
  const initialized = useRef(false);
  useEffect(() => {
    if (!isTauri()) return;
    const window = getCurrentWindow();
    let timer: ReturnType<typeof setTimeout>;
    const save = async (simple: boolean) => {
      try {
        const scale = await window.scaleFactor();
        const size = (await window.innerSize()).toLogical(scale);
        const position = (await window.outerPosition()).toLogical(scale);
        const value: SavedWindow = {
          x: position.x,
          y: position.y,
          width: size.width,
          height: size.height,
          maximized: await window.isMaximized(),
        };
        localStorage.setItem(key(simple), JSON.stringify(value));
      } catch {}
    };
    const restore = async (simple: boolean) => {
      try {
        const raw = localStorage.getItem(key(simple));
        if (!raw) return;
        const value = JSON.parse(raw) as SavedWindow;
        if (value.width < 900 || value.height < 660) return;
        if (await window.isMaximized()) await window.unmaximize();
        await window.setSize(new LogicalSize(value.width, value.height));
        await window.setPosition(new LogicalPosition(value.x, value.y));
        if (value.maximized) await window.maximize();
      } catch {}
    };
    const schedule = () => {
      clearTimeout(timer);
      timer = setTimeout(() => void save(mode.current), 250);
    };
    void (async () => {
      if (initialized.current) await save(mode.current);
      mode.current = simpleMode;
      await restore(simpleMode);
      initialized.current = true;
    })();
    const listeners = Promise.allSettled([
      window.onMoved(schedule),
      window.onResized(schedule),
    ]);
    return () => {
      clearTimeout(timer);
      void save(mode.current);
      void listeners.then((results) =>
        results.forEach((result) => {
          if (result.status === "fulfilled") result.value();
        }),
      );
    };
  }, [simpleMode]);
}
