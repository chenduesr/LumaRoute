import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import mark from "../assets/mark.svg";
export function Titlebar({ onError }: { onError: (message: string) => void }) {
  const action = async (
    name: "minimize" | "toggleMaximize" | "close" | "startDragging",
  ) => {
    if (!isTauri()) return;
    try {
      await getCurrentWindow()[name]();
    } catch (error) {
      onError(`窗口操作失败：${String(error)}`);
    }
  };
  return (
    <header className="titlebar">
      <div
        className="drag-region"
        onMouseDown={(e) => {
          if (e.button === 0 && e.detail === 1) void action("startDragging");
        }}
        onDoubleClick={() => void action("toggleMaximize")}
      >
        <img src={mark} alt="" width="18" height="18" />
        <span>LumaRoute</span>
      </div>
      {isTauri() ? (
        <div className="window-controls">
          <button aria-label="最小化" onClick={() => void action("minimize")}>
            <Minus size={14} />
          </button>
          <button
            aria-label="最大化或还原"
            onClick={() => void action("toggleMaximize")}
          >
            <Square size={12} />
          </button>
          <button
            aria-label="关闭窗口"
            className="window-close"
            onClick={() => void action("close")}
          >
            <X size={17} />
          </button>
        </div>
      ) : (
        <span className="preview-label">浏览器预览</span>
      )}
    </header>
  );
}
