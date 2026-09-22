import React from "react";
import ReactDOM from "react-dom/client";

import { createTransport } from "./api/create";
import App from "./App";
import { AppStore } from "./store/app";
import { StoreContext } from "./store/useStore";

const store = new AppStore(createTransport());
store.start();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <StoreContext.Provider value={store}>
      <App />
    </StoreContext.Provider>
  </React.StrictMode>,
);
