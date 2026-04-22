import { reactRouter } from "@react-router/dev/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import tsconfigPaths from "vite-tsconfig-paths";
import path from "path";

export default defineConfig({
  clearScreen: false,
  server: {
    allowedHosts: ['.bluetext.localhost'],
    // inotify doesn't propagate through hostPath mounts on macOS→k3d, so file
    // changes on the host never fire HMR. Polling is the workaround.
    watch: { usePolling: true, interval: 500 },
    proxy: {
      '/api': {
        target: process.env.API_URL || 'http://api',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, ''),
        headers: process.env.API_HOST ? { Host: process.env.API_HOST } : {},
      },
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, '.'),
      '~': path.resolve(__dirname, './app'),
    },
  },
  plugins: [tailwindcss(), reactRouter(), tsconfigPaths()],
});
