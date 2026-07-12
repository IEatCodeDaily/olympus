import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  define: {
    "process.env.NODE_ENV": JSON.stringify("development"),
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: false,
    include: ["src/**/*.test.{ts,tsx}"],
    // Increase global test timeout: Milkdown and CodeMirror editor tests legitimately
    // take 30 s+ in jsdom (StrictMode double-mount, batched fixture loop, suggestion
    // waits). The default 5 s causes spurious timeouts on slow machines when other
    // workers are saturating CPU/RAM.
    testTimeout: 30_000,
    server: {
      deps: {
        // Let Vite pre-bundle React so NODE_ENV=development resolves to dev builds
        inline: [/@testing-library/, "react", "react-dom", "react/jsx-runtime"],
      },
    },
  },
});
