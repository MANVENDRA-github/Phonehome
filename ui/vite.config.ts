import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    // dev-mode only: forward API calls to a locally running daemon
    proxy: { "/api": "http://localhost:8480" },
  },
});
