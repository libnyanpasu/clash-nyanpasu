export default {
  'scripts/**/*.{ts,tsx}': [
    'deno fmt --config scripts/deno.jsonc',
    'deno check --config scripts/deno.jsonc',
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
    // function tasks do not get the staged file list appended by lint-staged
    const files = filtered.join(' ')
    return [
      `prettier --write ${files}`,
      `oxlint --fix --no-error-on-unmatched-pattern ${files}`,
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
  '*.{css,html,less}': ['prettier --write', 'stylelint --fix'],
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
