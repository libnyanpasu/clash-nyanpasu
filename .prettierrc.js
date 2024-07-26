// @ts-check
import IanvsSorImportsPlugin from "@ianvs/prettier-plugin-sort-imports";

/** @type {import("prettier").Config} */
export default {
  endOfLine: "lf",
  semi: true,
  singleQuote: false,
  bracketSpacing: true,
  tabWidth: 2,
  trailingComma: "all",
  overrides: [
    {
      files: ["tsconfig.json", "jsconfig.json"],
      options: {
        parser: "jsonc",
      },
    },
  ],
  importOrder: [
    "^@ui/(.*)$",
    "^@interface/(.*)$",
    "^@/(.*)$",
    "^@(.*)$",
    "^[./]",
  ],
  importOrderParserPlugins: ["typescript", "jsx", "decorators-legacy"],
  importOrderTypeScriptVersion: "5.0.0",
  plugins: [
    IanvsSorImportsPlugin,
    "prettier-plugin-tailwindcss",
    "prettier-plugin-toml",
  ],
};
