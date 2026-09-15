import { useCallback, useState } from "react";
export function usePersistentState<T>(
  key: string,
  fallback: T,
  validate: (value: unknown) => value is T,
) {
  const [initial] = useState(() => {
    try {
      const raw = localStorage.getItem(key);
      if (raw === null) return { value: fallback, error: "" };
      const value: unknown = JSON.parse(raw);
      if (!validate(value)) throw new Error("Invalid stored data");
      return { value, error: "" };
    } catch {
      return {
        value: fallback,
        error: "本地数据无法读取，已使用默认值。原数据在你保存修改前会保留。",
      };
    }
  });
  const [value, setValue] = useState<T>(initial.value);
  const [error, setError] = useState(initial.error);
  const save = useCallback(
    (next: T) => {
      try {
        localStorage.setItem(key, JSON.stringify(next));
        setValue(next);
        setError("");
        return true;
      } catch {
        setError("无法保存到本地，请检查磁盘空间或存储权限后重试。");
        return false;
      }
    },
    [key],
  );
  return { value, save, error };
}
