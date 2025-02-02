export default {
  '*.{js,cjs,.mjs,jsx}': ['prettier --write', 'eslint --cache --fix'],
  'scripts/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p scripts/tsconfig.json --noEmit',
  ],
  'frontend/interface/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p frontend/interface/tsconfig.json --noEmit',
  ],
  'frontend/ui/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p frontend/ui/tsconfig.json --noEmit',
  ],
  'frontend/nyanpasu/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p frontend/nyanpasu/tsconfig.json --noEmit',
  ],
  'backend/**/*.{rs,toml}': [
    () =>
      'cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features',
    () => 'cargo fmt --manifest-path ./backend/Cargo.toml --all',
    // () => 'cargo test --manifest-path=./backend/Cargo.toml',
    // () => "cargo fmt --manifest-path=./backend/Cargo.toml --all",
    () => 'git add -u',
  ],
  '*.{html,sass,scss,less}': ['prettier --write', 'stylelint --fix'],
  'package.json': ['prettier --write'],
  '*.{md,json,jsonc,json5,yaml,yml,toml}': ['prettier --write'],
}
