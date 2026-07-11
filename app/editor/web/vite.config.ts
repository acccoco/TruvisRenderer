import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
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
