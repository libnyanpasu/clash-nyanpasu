export default {
  '*.{js,cjs,.mjs,jsx}': (filenames) => {
    const configFiles = [
      'eslint.config.js',
      '.lintstagedrc.js',
      'commitlint.config.js',
    ]
    const filtered = filenames.filter(
      (file) => !configFiles.some((config) => file.endsWith(config)),
    )
    if (filtered.length === 0) return []
    return ['prettier --write', 'eslint --cache --fix']
  },
  'scripts/**/*.{ts,tsx}': [
    'prettier --write',
    'node ./node_modules/eslint/bin/eslint.js --cache --fix',
    () => 'tsc -p scripts/tsconfig.json --noEmit',
  ],
  'frontend/interface/**/*.{ts,tsx}': [
    'prettier --write',
    'node ./node_modules/eslint/bin/eslint.js --cache --fix',
    () => 'tsc -p frontend/interface/tsconfig.json --noEmit',
  ],
  'frontend/ui/**/*.{ts,tsx}': [
    'prettier --write',
    'node ./node_modules/eslint/bin/eslint.js --cache --fix',
    () => 'tsc -p frontend/ui/tsconfig.json --noEmit',
  ],
  'frontend/nyanpasu/**/*.{ts,tsx}': [
    'prettier --write',
    'node ./node_modules/eslint/bin/eslint.js --cache --fix',
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
