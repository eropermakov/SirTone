// Butterchurn ships no types; the engine wraps it (`milkdrop.ts`).
declare module "butterchurn" {
  const butterchurn: unknown;
  export default butterchurn;
}

// The generated MilkDrop presets (`tools/preset-indexer`): name → preset with compiled equations.
declare module "*/milkdrop/presets.js" {
  const presets: Record<string, unknown>;
  export default presets;
}
