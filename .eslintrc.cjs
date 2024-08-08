module.exports = {
  root: true,
  env: {
    browser: true,
    node: true,
  },
  extends: [
    "plugin:react/recommended",
    "plugin:@typescript-eslint/recommended",
    "prettier",
    "plugin:prettier/recommended",
  ],
  ignorePatterns: ["index.html", "node_modules/", "dist/", "backend/**/target"],
  parser: "@typescript-eslint/parser",
  plugins: ["@typescript-eslint", "react-compiler", "react-hooks"],
  rules: {
    "no-console": process.env.NODE_ENV === "production" ? "error" : "off",
    "no-debugger": process.env.NODE_ENV === "production" ? "error" : "off",
    "@typescript-eslint/no-unused-vars": "warn",
    "@typescript-eslint/no-explicit-any": "warn",
    "react/jsx-no-undef": "off",
    "react/react-in-jsx-scope": "off",
    "@typescript-eslint/no-namespace": "off",
    "react-compiler/react-compiler": "error",
    "react-hooks/rules-of-hooks": "error",
    "react-hooks/exhaustive-deps": "warn",
    "react/no-children-prop": "off",
  },
  settings: {
    react: {
      version: "detect",
    },
    "import/resolver": {
      alias: {
        map: [
          ["@", "./src"],
          ["~", "./"],
        ],
        extensions: [".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs"],
      },
    },
  },
};
