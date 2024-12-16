/** @type {import("prettier").Config} */
module.exports = {
  endOfLine: 'lf',
  semi: false,
  singleQuote: true,
  bracketSpacing: true,
  tabWidth: 2,
  trailingComma: 'all',
  overrides: [
    {
      files: ['tsconfig.json', 'jsconfig.json'],
      options: {
        parser: 'jsonc',
      },
    },
  ],
  importOrder: [
    '^@nyanpasu/ui/(.*)$',
    '^@nyanpasu/interface/(.*)$',
    '^@/(.*)$',
    '^@(.*)$',
    '^[./]',
  ],
  importOrderParserPlugins: ['typescript', 'jsx', 'decorators-legacy'],
  importOrderTypeScriptVersion: '5.0.0',
  plugins: [
    '@ianvs/prettier-plugin-sort-imports',
    'prettier-plugin-tailwindcss',
    'prettier-plugin-toml',
  ],
}
