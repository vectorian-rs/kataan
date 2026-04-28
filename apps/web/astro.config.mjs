import { defineConfig } from 'astro/config';

const apiTarget = process.env.KATAAN_API_PROXY_TARGET ?? 'http://127.0.0.1:3001';

export default defineConfig({
  output: 'static',
  server: {
    host: '127.0.0.1',
    port: 3000,
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
