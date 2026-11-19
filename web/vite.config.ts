import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { VitePWA } from 'vite-plugin-pwa'

const API = process.env.WEATHER_API ?? 'http://127.0.0.1:8787'

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['favicon.svg', 'apple-touch-icon-180x180.png'],
      manifest: {
        name: 'Weather',
        short_name: 'Weather',
        description: 'Local-first weather dashboard with alerts, tides and marine data',
        theme_color: '#0ea5e9',
        background_color: '#0b1220',
        display: 'standalone',
        start_url: '/',
        icons: [
          { src: 'pwa-64x64.png', sizes: '64x64', type: 'image/png' },
          { src: 'pwa-192x192.png', sizes: '192x192', type: 'image/png' },
          { src: 'pwa-512x512.png', sizes: '512x512', type: 'image/png' },
          { src: 'maskable-icon-512x512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
        ],
      },
      workbox: {
        globPatterns: ['**/*.{js,css,html,svg,png,wasm}'],
        // sqlite-wasm ships helpers for modes we don't use (Worker1 API, OPFS proxy).
        globIgnores: ['**/sqlite3-worker1-*.js', '**/sqlite3-opfs-async-proxy-*.js'],
        // Weather responses: network first, fall back to the last copy when offline.
        runtimeCaching: [
          {
            urlPattern: ({ url }) => /^\/api\/(weather|alerts|marine)\b/.test(url.pathname),
            handler: 'NetworkFirst',
            options: {
              cacheName: 'weather-api',
              networkTimeoutSeconds: 8,
              expiration: { maxEntries: 60, maxAgeSeconds: 24 * 60 * 60 },
              cacheableResponse: { statuses: [200] },
            },
          },
        ],
        // The SQLite WASM binary is larger than Workbox's 2 MB default.
        maximumFileSizeToCacheInBytes: 5 * 1024 * 1024,
        // Never answer API or MCP requests with the cached app shell.
        navigateFallbackDenylist: [/^\/api\//, /^\/mcp/, /^\/sse/],
      },
    }),
  ],
  // sqlite-wasm loads its .wasm via import.meta.url; pre-bundling breaks that.
  optimizeDeps: { exclude: ['@sqlite.org/sqlite-wasm'] },
  worker: { format: 'es' },
  server: {
    port: 5173,
    proxy: {
      '/api': API,
      '/mcp': API,
      '/sse': API,
    },
  },
})
