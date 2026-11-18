export const DEFAULT_TERMINAL_FONT_SIZE = 15;
export const MIN_TERMINAL_FONT_SIZE = 10;
export const MAX_TERMINAL_FONT_SIZE = 32;

const STORAGE_KEY = "hshell.terminalFontSize";
const listeners = new Set<(size: number) => void>();

function readFontSize(): number {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    const size = stored?.trim() ? Number(stored) : NaN;
    if (
      Number.isInteger(size) &&
      size >= MIN_TERMINAL_FONT_SIZE &&
      size <= MAX_TERMINAL_FONT_SIZE
    )
      return size;
  } catch {
    // 存储不可用时仍允许本次运行调整字号。
  }
  return DEFAULT_TERMINAL_FONT_SIZE;
}

let fontSize = readFontSize();

export function getTerminalFontSize(): number {
  return fontSize;
}

export function setTerminalFontSize(size: number): void {
  if (!Number.isFinite(size)) return;
  const next = Math.min(
    MAX_TERMINAL_FONT_SIZE,
    Math.max(MIN_TERMINAL_FONT_SIZE, Math.round(size)),
  );
  if (next === fontSize) return;
  fontSize = next;
  try {
    localStorage.setItem(STORAGE_KEY, String(next));
  } catch {
    // 写入失败不阻断当前会话中的字号更新。
  }
  for (const listener of listeners) listener(next);
}

export function onTerminalFontSizeChange(
  listener: (size: number) => void,
): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function terminalFontShortcut(
  event: Pick<
    KeyboardEvent,
    "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "isComposing"
  >,
): "increase" | "decrease" | "reset" | null {
  if (!event.ctrlKey || event.altKey || event.metaKey || event.isComposing)
    return null;
  if (event.key === "+" || (event.key === "=" && !event.shiftKey))
    return "increase";
  if (!event.shiftKey && event.key === "-") return "decrease";
  if (!event.shiftKey && event.key === "0") return "reset";
  return null;
}
