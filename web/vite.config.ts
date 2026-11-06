import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  // Honour an assigned PORT so several instances can run side by side.
  server: {
    port: Number(process.env.PORT) || 5173,
    // Same origin as the page, so the console needs no CORS grant in
    // development and the daemon's allowlist covers only the built one.
    proxy: {
      '/api': {
        target: process.env.VEIL_DAEMON || 'http://127.0.0.1:8402',
        changeOrigin: true,
      },
    },
  },
  css: {
    modules: {
      // Readable in devtools, hashed enough to stay collision-free.
      generateScopedName: '[name]__[local]__[hash:base64:5]',
    },
  },
})
