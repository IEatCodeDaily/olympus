import React from "react";
import ReactDOM from "react-dom/client";
import { RouterProvider } from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { router } from "./router";
import { useLiveSync } from "./hooks/queries";
import { ThemeProvider } from "./theme";
import { AuthGate, useHallAuth } from "./auth";
// Design system: tokens (colors, type, spacing, radius, motion, fonts) + base
// resets + .ol-* component classes. Imported before index.css so the app-shell
// aliases in index.css can reference the design-system tokens.
import "./design/styles.css";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
});

function Root() {
  return <AuthGate><AuthenticatedApp /></AuthGate>;
}

function AuthenticatedApp() {
  const { organization } = useHallAuth();
  useLiveSync(organization.id);
  return <RouterProvider router={router} />;
}

// Bootstrap: start MSW mock worker (dev/e2e only) before React mounts.
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
    const { installWsMock } = await import("./mocks/ws-mock");
    installWsMock();
  }

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider>
          <Root />
        </ThemeProvider>
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

bootstrap();
