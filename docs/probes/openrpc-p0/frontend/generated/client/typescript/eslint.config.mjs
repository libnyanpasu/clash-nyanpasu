import eslint from '@eslint/js';
import tseslint from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';
import prettierConfig from 'eslint-config-prettier';
import prettierPlugin from 'eslint-plugin-prettier';
import { fileURLToPath } from 'url';
import { dirname } from 'path';
import globals from "globals";


const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

export default [
  eslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        project: ['./tsconfig.json'],
        tsconfigRootDir: __dirname,
        ecmaVersion: 2022,
        sourceType: 'module',
        ecmaFeatures: {
        }
      },
      globals: {
        ...globals.node,
        ...globals.es6,
        chrome: 'readonly',
        browser: 'readonly'
      }
    },
    plugins: {
      '@typescript-eslint': tseslint,
      'prettier': prettierPlugin
    },
    rules: {
      ...tseslint.configs['recommended'].rules,
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/explicit-module-boundary-types': 'off',
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unused-vars': ['warn', { argsIgnorePattern: '^_' }],
      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'prettier/prettier': 'error'
    },
    ignores: ["package.json","node_modules/", "**/*/dist/", "build/", "coverage/","*.config.js", "**/*.config.ts", "vitest.config.ts", "vite.config.ts"],
    settings: {
      react: {
        version: 'detect'
      }
    }
  },
  prettierConfig
];