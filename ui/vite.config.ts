import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

// Resolve env from (in precedence order) shell `process.env`, then `.env.local`,
// `.env.[mode]`, `.env` — Vite's standard `loadEnv`. We keep a `define` block so
// the three VITE_* keys always have a concrete value (mock-friendly defaults)
// even when no env file is present, but real `.env.local` / shell values win.
export default defineConfig(({ mode }) => {
  const fileEnv = loadEnv(mode, process.cwd(), "VITE_");
  const pick = (key: string, fallback: string) =>
    process.env[key] ?? fileEnv[key] ?? fallback;

  return {
    plugins: [react()],
    server: {
      port: 5177,
      host: "127.0.0.1",
    },
    define: {
      // Defaults keep the mock dev experience working with zero config; a
      // `.env.local` (or shell var) pointing at a real backend overrides them.
      "import.meta.env.VITE_USE_MOCKS": JSON.stringify(
        pick("VITE_USE_MOCKS", "true")
      ),
      "import.meta.env.VITE_API_BASE": JSON.stringify(
        pick("VITE_API_BASE", "http://127.0.0.1:8787")
      ),
      "import.meta.env.VITE_API_TOKEN": JSON.stringify(
        pick("VITE_API_TOKEN", "dev-mock-token")
      ),
    },
  };
});
