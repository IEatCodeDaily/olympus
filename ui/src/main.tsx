import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// Start MSW mock worker before React mounts (dev only)
async function bootstrap() {
  const useMocks = import.meta.env.VITE_USE_MOCKS !== "false";
  if (useMocks) {
    const { worker } = await import("./mocks/browser");
    await worker.start({
      onUnhandledRequest: "bypass",
      serviceWorker: {
        url: "/mockServiceWorker.js",
      },
    });
  }

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
}

bootstrap();
