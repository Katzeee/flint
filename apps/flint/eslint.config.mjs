import { appConfig } from "@cairn/lint/eslint";
import tseslint from "typescript-eslint";

export default [
  { ignores: ["dist/**", "cairn/**", "node_modules/**", "src-tauri/**"] },
  { files: ["src/**/*.{ts,tsx}"], languageOptions: { parser: tseslint.parser } },
  { files: ["src/**/*.{ts,tsx}"], ...appConfig },
];
