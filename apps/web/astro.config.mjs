import { defineConfig } from 'astro/config';

const apiTarget = process.env.KATAAN_API_PROXY_TARGET ?? 'http://127.0.0.1:3001';
const webHost = process.env.KATAAN_WEB_HOST ?? '127.0.0.1';
const webPort = Number.parseInt(process.env.KATAAN_WEB_PORT ?? '3000', 10);

// The web app is a client-routed SPA: every route renders the same shell and
// `lib/dashboard` reads window.location to decide what to load from the API.
// So the build is fully static (one index.html + assets), and deep links are
// handled by serving the shell for any non-API, non-asset path. In production
// the Rust server does that (see crates/kataan-server); in `astro dev` this
// integration does it, so hard-refreshing a deep link still works.
const spaFallbackDev = {
  name: 'kataan:spa-fallback-dev',
  hooks: {
    'astro:server:setup': ({ server }) => {
      server.middlewares.use((req, _res, next) => {
        const url = req.url ?? '/';
        // What counts as an asset is named, not guessed from an extension.
        //
        // A file route is a vault path — `/docs/report.pdf` — so "has a dot"
        // would send every one of them to a 404 here while production served
        // the shell. The Rust fallback looks the path up among embedded assets
        // and serves the shell on a miss; this is the same rule, expressed
        // against the paths Vite actually owns.
        const isAsset =
          url.startsWith('/@') ||
          url.startsWith('/node_modules') ||
          url.startsWith('/_astro/') ||
          url.startsWith('/src/') ||
          url === '/favicon.ico';
        if (req.method === 'GET' && url !== '/' && !url.startsWith('/api') && !isAsset) {
          req.url = '/';
        }
        next();
      });
    },
  },
};

export default defineConfig({
  output: 'static',
  integrations: [spaFallbackDev],
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
