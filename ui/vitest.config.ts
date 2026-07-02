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
    server: {
      deps: {
        // Let Vite pre-bundle React so NODE_ENV=development resolves to dev builds
        inline: [/@testing-library/, "react", "react-dom", "react/jsx-runtime"],
      },
    },
  },
});
