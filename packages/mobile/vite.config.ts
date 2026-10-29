import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";
import { resolve } from "node:path";
export default defineConfig({
  plugins: [vue()],
  publicDir: false,
  resolve: {
    alias: {
      "@wonderful-ui/parser": resolve(__dirname, "../parser/src/index.ts"),
    },
  },
  build: { target: "es2020", sourcemap: false },
});
