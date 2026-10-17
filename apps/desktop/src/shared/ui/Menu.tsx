import { ChevronRight, type LucideIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/shared/lib/utils";

export type MenuItem =
  | {
      id: string;
      label: string;
      icon?: LucideIcon;
      /** Hotkey hint on the right (`shortcut`, text-tertiary). */
      shortcut?: string;
      danger?: boolean;
      disabled?: boolean;
      /** Opens a nested flyout («Добавить в плейлист ›»). */
      submenu?: MenuItem[];
      onSelect?: () => void;
    }
  | { separator: true };

type Props = {
  items: MenuItem[];
  /** Viewport point the menu grows from (pointer or the «…» button). */
  at: { x: number; y: number };
  label: string;
  onClose: () => void;
  /** A nested menu closes the whole tree when an item is chosen. */
  onChosen?: () => void;
};

const isItem = (i: MenuItem): i is Exclude<MenuItem, { separator: true }> => !("separator" in i);

/**
 * MenuFlyout (DESIGN.md §9; WAI-ARIA menu pattern): `bg-flyout`, border, radius-lg, shadow, 6 px
 * padding, 240–320 wide; 40 px pill items with icon 20 + 12 + `menu`; separators; destructive item
 * at the bottom in `danger`. Opens from the point of call in 150 ms, stays inside the window;
 * ↑/↓/Home/End move the highlight, Enter/Space choose, → opens a submenu, ←/Esc close.
 */
export function Menu({ items, at, label, onClose, onChosen }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState(at);
  const [active, setActive] = useState(-1);
  const [sub, setSub] = useState<{ index: number; at: { x: number; y: number } } | null>(null);
  const enabled = items.map((i, n) => (isItem(i) && !i.disabled ? n : -1)).filter((n) => n >= 0);

  // Keep the whole flyout inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      x: Math.max(8, Math.min(at.x, window.innerWidth - r.width - 8)),
      y: Math.max(8, Math.min(at.y, window.innerHeight - r.height - 8)),
    });
    el.focus();
  }, [at]);

  useEffect(() => {
    const down = (e: PointerEvent) => {
      if (!(e.target instanceof Node) || !ref.current?.parentElement?.contains(e.target)) onClose();
    };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  const choose = (n: number) => {
    const it = items[n];
    if (!it || !isItem(it) || it.disabled) return;
    if (it.submenu) {
      const row = ref.current?.querySelectorAll<HTMLElement>("[role=menuitem]")[n];
      const r = row?.getBoundingClientRect();
      setSub({ index: n, at: { x: (r?.right ?? at.x) + 4, y: r?.top ?? at.y } });
      return;
    }
    (onChosen ?? onClose)();
    it.onSelect?.();
  };

  const move = (dir: 1 | -1) => {
    if (enabled.length === 0) return;
    const at = enabled.indexOf(active);
    const next =
      at < 0 ? (dir > 0 ? 0 : enabled.length - 1) : (at + dir + enabled.length) % enabled.length;
    setActive(enabled[next] ?? -1);
  };

  const subItems = sub ? items[sub.index] : undefined;

  return (
    <div className="fixed inset-0 z-(--z-overlay)" style={{ pointerEvents: "none" }}>
      <div
        ref={ref}
        role="menu"
        aria-label={label}
        tabIndex={-1}
        onContextMenu={(e) => e.preventDefault()}
        onKeyDown={(e) => {
          const keys: Record<string, () => void> = {
            ArrowDown: () => move(1),
            ArrowUp: () => move(-1),
            Home: () => setActive(enabled[0] ?? -1),
            End: () => setActive(enabled.at(-1) ?? -1),
            Enter: () => choose(active),
            " ": () => choose(active),
            ArrowRight: () => {
              const it = items[active];
              if (it && isItem(it) && it.submenu) choose(active);
            },
            ArrowLeft: onClose,
            Escape: onClose,
            Tab: onClose,
          };
          const f = keys[e.key];
          if (f) {
            e.preventDefault();
            e.stopPropagation();
            f();
          }
        }}
        className="flyout-in absolute flex w-60 max-w-80 flex-col rounded-(--radius-lg) border border-border bg-bg-flyout p-1.5 shadow-(--shadow-flyout) outline-none"
        style={{ left: pos.x, top: pos.y, pointerEvents: "auto" }}
      >
        {items.map((it, n) =>
          isItem(it) ? (
            <div
              key={it.id}
              role="menuitem"
              aria-disabled={it.disabled || undefined}
              aria-haspopup={it.submenu ? "menu" : undefined}
              tabIndex={-1}
              onPointerEnter={() => setActive(n)}
              onClick={() => choose(n)}
              onKeyDown={() => {}}
              className={cn(
                "t-menu flex h-(--item-height) shrink-0 cursor-default items-center gap-3 rounded-(--radius-md) px-3",
                n === active && "bg-bg-hover",
                it.danger ? "text-danger" : "text-text-primary",
                it.disabled && "opacity-40",
              )}
            >
              {it.icon && (
                <it.icon
                  size={20}
                  strokeWidth={1.5}
                  className={it.danger ? "text-danger" : "text-icon"}
                />
              )}
              <span className="min-w-0 flex-1 truncate">{it.label}</span>
              {it.shortcut && <span className="t-shortcut text-text-tertiary">{it.shortcut}</span>}
              {it.submenu && <ChevronRight size={16} strokeWidth={1.5} className="text-icon" />}
            </div>
          ) : (
            // biome-ignore lint/suspicious/noArrayIndexKey: separators have no identity
            <hr key={`sep-${n}`} className="mx-3 my-1.5 h-px border-0 bg-separator" />
          ),
        )}
      </div>
      {sub && subItems && isItem(subItems) && subItems.submenu && (
        <Menu
          items={subItems.submenu}
          at={sub.at}
          label={subItems.label}
          onChosen={onChosen ?? onClose}
          onClose={() => {
            setSub(null);
            ref.current?.focus();
          }}
        />
      )}
    </div>
  );
}
