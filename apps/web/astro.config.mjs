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
        // Any request with a file extension (incl. /_astro/*.js|css) or a Vite
        // internal path is a real asset, not a page to fall back for.
        //
        // Except a `~`-prefixed route, which is a view rather than a document
        // and may legitimately carry a dot: `/~file/docs/report.pdf` names a
        // file *inside the vault*, served by the API, not an asset of this
        // server. The Rust fallback serves the shell for it, and this has to
        // agree or a deep link would work in production and 404 in dev.
        const isViewRoute = url.startsWith('/~');
        const isAsset =
          !isViewRoute &&
          (url.startsWith('/@') ||
            url.startsWith('/node_modules') ||
            /\.[a-zA-Z0-9]+(\?|$)/.test(url));
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
