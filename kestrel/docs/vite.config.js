import { defineConfig } from 'vite';

export default defineConfig({
  root: 'kestrel/docs',
  server: { host: '127.0.0.1', port: 3000 },
  build: { assetsInlineLimit: 0 },
});
