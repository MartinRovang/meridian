import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// The app never fetches a relative path, so there is no /api proxy here on purpose:
// `pnpm dev` is pointed at a server with ?api=...&token=... in the URL, exactly like the
// packaged app does it.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**', '**/crates/**'] } },
  build: { outDir: 'dist' },
})
