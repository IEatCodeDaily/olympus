import React from "react";
import ReactDOM from "react-dom/client";
import { ConvexProvider, ConvexReactClient } from "convex/react";
import { App } from "./App";

const convexUrl = import.meta.env.VITE_CONVEX_URL as string | undefined;
const convex = new ConvexReactClient(convexUrl ?? "http://127.0.0.1:3210");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConvexProvider client={convex}>
      <App />
    </ConvexProvider>
  </React.StrictMode>,
);
