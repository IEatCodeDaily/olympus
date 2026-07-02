import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => {
  const fileEnv = loadEnv(mode, process.cwd(), "VITE_");
  const pick = (key: string, fallback: string) =>
    process.env[key] ?? fileEnv[key] ?? fallback;

  // Backend target for the dev proxy. Always a real URL, never empty.
  const proxyTarget = pick("VITE_PROXY_TARGET", "http://127.0.0.1:8799");

  return {
    plugins: [react()],
    server: {
      port: 5177,
      host: "127.0.0.1",
      proxy: {
        "/api": { target: proxyTarget, changeOrigin: true },
        "/ws": { target: proxyTarget, changeOrigin: true, ws: true },
      },
    },
    define: {
      // In dev, the UI talks same-origin (/api → proxied). In production builds,
      // VITE_API_BASE can be set to the real backend URL.
      "import.meta.env.VITE_USE_MOCKS": JSON.stringify(
        pick("VITE_USE_MOCKS", "false")
      ),
      "import.meta.env.VITE_API_BASE": JSON.stringify(
        pick("VITE_API_BASE", "")
      ),
      "import.meta.env.VITE_API_TOKEN": JSON.stringify(
        pick("VITE_API_TOKEN", "")
      ),
    },
  };
});
