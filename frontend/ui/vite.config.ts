import { defineConfig } from 'vite'
import dts from 'vite-plugin-dts'
import tsconfigPaths from 'vite-tsconfig-paths'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [dts({ rollupTypes: true }), react(), tsconfigPaths()],
  build: {
    lib: {
      entry: 'src/index.ts',
      fileName: 'index',
      formats: ['es'],
    },
    rollupOptions: {
      external: ['react', 'react-dom', '@tauri-apps/api'],
      output: {
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
          OS_PLATFORM: 'OS_PLATFORM',
        },
      },
    },
  },
})
