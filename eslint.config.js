// ESLint is used for exactly one rule Biome lacks: no literal strings in JSX (docs/03 §3.2, T0.8).
import i18next from "eslint-plugin-i18next";
import tseslint from "typescript-eslint";

export default [
  { ignores: ["target/**", "**/node_modules/**", "**/dist/**", "spikes/**", "docs/**"] },
  {
    files: ["apps/desktop/src/**/*.tsx"],
    languageOptions: { parser: tseslint.parser },
    plugins: { i18next },
    rules: { "i18next/no-literal-string": ["error", { mode: "jsx-only" }] },
  },
];
