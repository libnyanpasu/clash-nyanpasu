import { KnipConfig } from 'knip'

export default {
  entry: [
    'frontend/nyanpasu/src/main.tsx',
    'frontend/nyanpasu/src/pages/**/*.tsx',
    'scripts/*.{js,ts}',
  ],
  project: ['frontend/**/*.{ts,js,jsx,tsx}', 'scripts/**/*.{js,ts}'],
} satisfies KnipConfig
