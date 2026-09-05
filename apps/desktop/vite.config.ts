import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    strictPort: true,
    proxy: {
      "/api/v1": "http://127.0.0.1:8080",
    },
  },
  build: {
    outDir: "web-dist",
    emptyOutDir: true,
    sourcemap: true,
  },
});
