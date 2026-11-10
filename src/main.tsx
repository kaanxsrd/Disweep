import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "sonner";
import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import "./styles.css";

try {
  Object.freeze(Object.prototype);
} catch {}

const queries = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: 60_000 } },
});

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={queries}>
        <ErrorBoundary scope="window">
          <App />
        </ErrorBoundary>
        <Toaster position="bottom-right" theme="system" richColors closeButton />
      </QueryClientProvider>
    </StrictMode>,
  );
}
