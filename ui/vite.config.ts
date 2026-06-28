import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5177,
    host: "127.0.0.1",
  },
  define: {
    // Default: mocks enabled in dev
    "import.meta.env.VITE_USE_MOCKS": JSON.stringify(
      process.env.VITE_USE_MOCKS ?? "true"
    ),
    "import.meta.env.VITE_API_BASE": JSON.stringify(
      process.env.VITE_API_BASE ?? "http://127.0.0.1:8787"
    ),
    "import.meta.env.VITE_API_TOKEN": JSON.stringify(
      process.env.VITE_API_TOKEN ?? "dev-mock-token"
    ),
  },
});
