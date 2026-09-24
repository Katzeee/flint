import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  resolve: { dedupe: ["react", "react-dom"] },
  build: {
    outDir: "dist",
    target: ["chrome120", "firefox120", "safari17"],
    rolldownOptions: {
      onwarn(warning, defaultHandler) {
        // Server/client directives have no effect in this client-only WebView.
        if (warning.code !== "MODULE_LEVEL_DIRECTIVE") defaultHandler(warning);
      },
    },
  },
});
