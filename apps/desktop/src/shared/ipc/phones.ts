import { commands, type PhoneInfo, type PhonesStatus } from "./bindings";

/** Hub «Телефоны» (stage 8, FR-HUB-001): the phones this PC sees right now. */
export type { PhoneInfo, PhonesStatus };

export function phonesList(): Promise<PhonesStatus> {
  return commands.phonesList();
}
