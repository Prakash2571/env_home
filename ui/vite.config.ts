import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const backend = process.env.CBL_DEV_BACKEND ?? "http://127.0.0.1:47440";

export default defineConfig({
  plugins: [react()],
  build: { outDir: "dist", emptyOutDir: true, sourcemap: false },
  server: {
    port: 5180,
    strictPort: true,
    proxy: {
      "/api": { target: backend, changeOrigin: false },
      "/health": { target: backend, changeOrigin: false },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./tests/support/setup.ts"],
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "json-summary"],
      include: ["src/**/*.ts", "src/**/*.tsx"],
      exclude: ["src/main.tsx", "src/api/types.ts"],
    },
  },
});
