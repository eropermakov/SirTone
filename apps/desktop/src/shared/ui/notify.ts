/**
 * Short info toasts for successful actions («Будет следующим: …») — DESIGN.md §1: every direct
 * action answers at once. One listener (the shell) shows them; anything may call `notify`.
 */
const EVENT = "sirtone:notify";

export function notify(text: string): void {
  window.dispatchEvent(new CustomEvent<string>(EVENT, { detail: text }));
}

export function onNotify(cb: (text: string) => void): () => void {
  const h = (e: Event) => cb((e as CustomEvent<string>).detail);
  window.addEventListener(EVENT, h);
  return () => window.removeEventListener(EVENT, h);
}
