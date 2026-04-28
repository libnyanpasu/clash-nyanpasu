import { KnipConfig } from 'knip'

export default {
  entry: [
    'frontend/nyanpasu/src/main.tsx',
    'frontend/nyanpasu/src/pages/**/*.tsx',
  ],
  project: ['frontend/**/*.{ts,js,jsx,tsx}'],
} satisfies KnipConfig
