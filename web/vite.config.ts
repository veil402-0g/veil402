import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  // Honour an assigned PORT so several instances can run side by side.
  server: { port: Number(process.env.PORT) || 5173 },
  css: {
    modules: {
      // Readable in devtools, hashed enough to stay collision-free.
      generateScopedName: '[name]__[local]__[hash:base64:5]',
    },
  },
})
