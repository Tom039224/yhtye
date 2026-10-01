import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // Stylesheets are empty in tests, except the ones a test reads as text (`import css from "./x.css?raw"`).
    css: { include: [/\.css\?raw$/] },
  },
});
