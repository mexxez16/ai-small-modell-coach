import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev server proxies API + WebSocket traffic to coach-api; the built dist/ is
// served by coach-api with SPA fallback in production. coach-api defaults to
// 8901 and scans upward on conflict; override here with COACH_PORT.
const apiPort = process.env.COACH_PORT ?? "8901";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": { target: `http://127.0.0.1:${apiPort}`, changeOrigin: true },
      "/ws": { target: `ws://127.0.0.1:${apiPort}`, ws: true },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
