export default {
  "*.{js,cjs,.mjs,jsx}": ["prettier --write", "eslint --cache --fix"],
  "*.{ts,tsx}": [
    "prettier --write",
    "eslint --cache --fix",
    () => "tsc -p tsconfig.json --noEmit",
  ],
  "backend/**/*.{rs,toml}": [
    () =>
      "cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features",
    () => "cargo fmt --manifest-path ./backend/Cargo.toml --all",
    // () => 'cargo test --manifest-path=./backend/Cargo.toml',
    // () => "cargo fmt --manifest-path=./backend/Cargo.toml --all",
  ],
  "*.{html,sass,scss,less}": ["prettier --write", "stylelint --fix"],
  "package.json": ["prettier --write"],
  "*.{md,json,jsonc,json5,yaml,yml,toml}": ["prettier --write"],
};
