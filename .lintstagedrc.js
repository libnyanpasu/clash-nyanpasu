export default {
  'scripts/deno/**/*.{ts,tsx}': [
    'deno fmt --config scripts/deno/deno.jsonc',
    'deno check --config scripts/deno/deno.jsonc',
  ],
  '*.{js,cjs,.mjs,jsx}': (filenames) => {
    const configFiles = [
      '.oxlintrc.json',
      '.lintstagedrc.js',
      'commitlint.config.js',
    ]
    const filtered = filenames.filter(
      (file) => !configFiles.some((config) => file.endsWith(config)),
    )
    if (filtered.length === 0) return []
    return ['prettier --write', 'oxlint --fix']
  },
  'scripts/**/*.{ts,tsx}': (filenames) => {
    const filtered = filenames.filter((file) => !file.includes('scripts/deno/'))
    if (filtered.length === 0) return []
    return [
      `prettier --write ${filtered.join(' ')}`,
      `oxlint --fix ${filtered.join(' ')}`,
      'tsc -p scripts/tsconfig.json --noEmit',
    ]
  },
  'frontend/interface/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
    () => 'tsc -p frontend/interface/tsconfig.json --noEmit',
  ],
  'frontend/utils/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
    () => 'tsc -p frontend/utils/tsconfig.json --noEmit',
  ],
  'frontend/nyanpasu/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
    () => 'tsc -p frontend/nyanpasu/tsconfig.json --noEmit',
  ],
  'backend/**/*.{rs,toml}': [
    () =>
      'cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features',
    () => 'cargo fmt --manifest-path ./backend/Cargo.toml --all',
    // () => 'cargo test --manifest-path=./backend/Cargo.toml',
    // () => "cargo fmt --manifest-path=./backend/Cargo.toml --all",
    // do not submit untracked files
    // () => 'git add -u',
  ],
  '*.{html,sass,scss,less}': ['prettier --write', 'stylelint --fix'],
  'package.json': ['prettier --write'],
  '*.{md,json,jsonc,json5,yaml,yml,toml}': (filenames) => {
    // exclude frontend/nyanpasu/messages directory
    const filtered = filenames.filter(
      (file) => !file.includes('frontend/nyanpasu/messages/'),
    )
    if (filtered.length === 0) return []
    return `prettier --write ${filtered.join(' ')}`
  },
}
