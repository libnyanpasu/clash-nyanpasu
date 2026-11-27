import path from 'node:path'
import Icons from 'unplugin-icons/vite'
import { defineConfig } from 'vite'
import tailwindPlugin from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  clearScreen: false,
  server: {
    port: 3001,
  },
  plugins: [
    tailwindPlugin(),
    react(),
    Icons({
      compiler: 'jsx',
    }),
  ],
  resolve: {
    alias: [{ find: '@', replacement: path.resolve('./src') }],
    dedupe: ['react', 'react-dom'],
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
