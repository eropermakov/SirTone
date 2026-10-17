/**
 * Keyboard event → global-hotkey accelerator («Ctrl+Alt+Right»), the format `hotkeys` settings and
 * tauri-plugin-global-shortcut use (FR-WIN-001 combination editor). `null` = not a usable global
 * combination yet: a bare modifier, or no Ctrl/Alt/Win (a global hotkey without them would steal
 * ordinary typing from every program).
 */
export function accelFromEvent(e: {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string | null {
  const key = keyName(e.code);
  if (!key) return null;
  if (!(e.ctrlKey || e.altKey || e.metaKey)) return null;
  const mods = [
    e.ctrlKey && "Ctrl",
    e.altKey && "Alt",
    e.shiftKey && "Shift",
    e.metaKey && "Super",
  ].filter(Boolean);
  return [...mods, key].join("+");
}

/** Physical key (layout-independent: the same combination in Russian and English layouts). */
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  const named: Record<string, string> = {
    Space: "Space",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    ArrowUp: "Up",
    ArrowDown: "Down",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    Insert: "Insert",
    Delete: "Delete",
    Comma: "Comma",
    Period: "Period",
    Slash: "Slash",
    Minus: "Minus",
    Equal: "Equal",
    BracketLeft: "BracketLeft",
    BracketRight: "BracketRight",
    Semicolon: "Semicolon",
    Quote: "Quote",
    Backquote: "Backquote",
    Backslash: "Backslash",
  };
  return named[code] ?? null;
}
