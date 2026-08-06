import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import adapter from "@sveltejs/adapter-static";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [
    sveltekit({
      adapter: adapter({ fallback: "index.html" }),
      alias: {
        "#lib": "src/lib",
      },
    }),
  ],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // User projects and local runtimes are runtime data, not DevPanel
      // source. Nested Vite configs otherwise stall the Svelte transport.
      ignored: [
        "**/src-tauri/**",
        "**/www/**",
        "**/data/**",
        "**/bin/**",
        "**/release/**",
      ],
    },
  },
}));
