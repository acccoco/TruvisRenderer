import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const repositoryRoot = fileURLToPath(new URL('../../../', import.meta.url));

export default defineConfig({
  plugins: [react()],
  server: {
    fs: {
      // Web UI 直接复用仓库级 DruvisIII 品牌资产，不在 frontend 内维护第二份副本。
      allow: [repositoryRoot],
    },
    port: 5173,
    strictPort: true,
    proxy: {
      '/api/editor': {
        target: 'http://127.0.0.1:9473',
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
