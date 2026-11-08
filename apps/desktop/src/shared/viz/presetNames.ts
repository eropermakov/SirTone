import names from "../../../../../assets/viz/milkdrop/names.json";

const NAMES = names as Record<string, string>;

/**
 * What the preset looks like, in words (owner, 2026-11-08): colour, light, motion — generated from
 * the measured descriptors by `tools/preset-names.py`. The author's own name stays as a caption.
 */
export const presetLabel = (name: string): string => NAMES[name] ?? name;
