import { defineConfig } from "vite";

// Tauri expects a fixed dev port and does not want Vite to clear the screen.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // Keep the bundle consumable by older WebKitGTK builds shipped on Linux distros.
  build: {
    target: "es2020",
    sourcemap: false,
  },
});
