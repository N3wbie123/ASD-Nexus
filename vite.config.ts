import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${process.env.ASD_NEXUS_PORT || 3850}`,
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist/web",
  },
});
