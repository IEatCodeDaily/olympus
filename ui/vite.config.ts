import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode, command }) => {
  const fileEnv = loadEnv(mode, process.cwd(), "VITE_");
  const pick = (key: string, fallback: string) =>
    process.env[key] ?? fileEnv[key] ?? fallback;

  // Backend target for the dev proxy. Always a real URL, never empty.
  const proxyTarget = pick("VITE_PROXY_TARGET", "http://127.0.0.1:8799");
  const allowedHosts = pick("VITE_ALLOWED_HOSTS", "")
    .split(",")
    .map((host) => host.trim())
    .filter(Boolean);
  const servingDevelopment = command === "serve";

  return {
    plugins: [react()],
    server: {
      port: 5177,
      host: "127.0.0.1",
      allowedHosts,
      proxy: {
        "/api": { target: proxyTarget, changeOrigin: true },
        "/ws": { target: proxyTarget, changeOrigin: true, ws: true },
      },
    },
    define: {
      // Never compile mock mode or an alternate Hall into production. Source
      // also checks development mode, but build configuration is the outer
      // fail-closed boundary.
      "import.meta.env.VITE_USE_MOCKS": JSON.stringify(
        servingDevelopment ? pick("VITE_USE_MOCKS", "false") : "false"
      ),
      "import.meta.env.VITE_API_BASE": JSON.stringify(
        servingDevelopment ? pick("VITE_API_BASE", "") : ""
      ),
    },
  };
});
