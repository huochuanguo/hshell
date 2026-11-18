import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/target/**", "**/src-tauri/**", "**/crates/**"] },
  },
  build: { target: "es2022" },
});
