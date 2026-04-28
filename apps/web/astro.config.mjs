import { defineConfig } from 'astro/config';

const apiTarget = process.env.KATAAN_API_PROXY_TARGET ?? 'http://127.0.0.1:3001';
const webHost = process.env.KATAAN_WEB_HOST ?? '127.0.0.1';
const webPort = Number.parseInt(process.env.KATAAN_WEB_PORT ?? '3000', 10);

export default defineConfig({
  output: 'static',
  server: {
    host: webHost,
    port: webPort,
  },
  vite: {
    server: {
      strictPort: true,
      proxy: {
        '/api': apiTarget,
      },
    },
  },
});
