import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import { createHostsRegistry } from "./store/bootstrap";
import { HostsProvider } from "./store/useHosts";

const registry = createHostsRegistry();
void registry.start();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <HostsProvider registry={registry}>
      <App />
    </HostsProvider>
  </React.StrictMode>,
);
