/**
 * Short info toasts for successful actions («Будет следующим: …») — DESIGN.md §1: every direct
 * action answers at once. One listener (the shell) shows them; anything may call `notify`. An
 * action («Отменить») makes the toast stay longer.
 */
const EVENT = "sirtone:notify";

export type NotifyAction = { label: string; onClick: () => void };
export type Notice = { text: string; action?: NotifyAction | undefined };

export function notify(text: string, action?: NotifyAction): void {
  window.dispatchEvent(new CustomEvent<Notice>(EVENT, { detail: { text, action } }));
}

export function onNotify(cb: (n: Notice) => void): () => void {
  const h = (e: Event) => cb((e as CustomEvent<Notice>).detail);
  window.addEventListener(EVENT, h);
  return () => window.removeEventListener(EVENT, h);
}
