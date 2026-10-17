import { coverUrl } from "@/shared/ui/Cover";
import { pickSlug } from "./autoPick";
import { ACCENT_SLUGS, type AccentSlug } from "./theme";

/**
 * «Авто» — the accent from the cover (T2.2; DESIGN.md §4 «Авто-режим», exactly):
 * 1) the cover at 64×64 in OKLab; 2) drop near-black (L < 0.12) and near-white (L > 0.95);
 * 3) k-means k = 5, dominant = max(share × (0.3 + chroma)); 4) chroma < 0.04 → «Серый»
 * («Чёрный» if L < 0.3 or > 0.85); 5) else the nearest (OKLab) of the 15 chromatic sets by
 * their light-theme fill; 6) cached per cover. Raw cover colours never reach the controls.
 */
let lightFills: Record<string, string> | null = null;
/** The 17 light-theme fills, read from the stylesheet (no copies of the tokens). */
function fills(): Record<string, string> {
  if (lightFills) return lightFills;
  const probe = document.createElement("div");
  probe.dataset.theme = "light";
  probe.hidden = true;
  document.body.appendChild(probe);
  const cs = getComputedStyle(probe);
  lightFills = Object.fromEntries(
    ACCENT_SLUGS.map((s) => [s, cs.getPropertyValue(`--accent-${s}-fill`).trim()]),
  );
  probe.remove();
  return lightFills;
}

const cache = new Map<string, AccentSlug>();
const KEY = (hash: string) => `sirtone.auto.${hash}`;

/** The «Авто» set for a cover (cached in memory and in this profile). */
export async function autoSlug(hash: string): Promise<AccentSlug> {
  const hit = cache.get(hash);
  if (hit) return hit;
  try {
    const stored = localStorage.getItem(KEY(hash)) as AccentSlug | null;
    if (stored && (ACCENT_SLUGS as readonly string[]).includes(stored)) {
      cache.set(hash, stored);
      return stored;
    }
  } catch {
    // no storage — compute
  }
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.decoding = "async";
  img.src = coverUrl(hash, 64);
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = 64;
  canvas.height = 64;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return "sirkov";
  ctx.drawImage(img, 0, 0, 64, 64);
  const slug = pickSlug(ctx.getImageData(0, 0, 64, 64).data, fills());
  cache.set(hash, slug);
  try {
    localStorage.setItem(KEY(hash), slug);
  } catch {
    // ignore
  }
  return slug;
}
