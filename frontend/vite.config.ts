import path from 'node:path'
import fs from 'node:fs'
import process from 'node:process'
import { defineConfig, loadEnv } from 'vite'
import vue from '@vitejs/plugin-vue'
import Components from 'unplugin-vue-components/vite'
import AutoImport from 'unplugin-auto-import/vite'

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd())
  // Use the deployed demo server for local UI previews when no local API is
  // configured. VITE_DEV_API still takes precedence for local development.
  const devApi = env.VITE_DEV_API || 'https://st.9888812.xyz'
  const workspaceRoot = path.resolve(__dirname, '..')
  return {
    resolve: {
      alias: {
        '@': path.resolve(__dirname, 'src'),
      },
    },
    base: './',
    build: {
      outDir: path.resolve(workspaceRoot, 'web'),
      emptyOutDir: false,
      assetsDir: 'theme-light-assets',
    },
    publicDir: fs.existsSync(path.resolve(__dirname, 'public'))
      ? path.resolve(__dirname, 'public')
      : false,
    plugins: [
      vue(),
      AutoImport({
        imports: ['vue'],
        dts: true,
        vueTemplate: true,
      }),
      Components({
        dts: true,
      }),
    ],
    server: {
      proxy: {
        '/json': devApi,
        '/api/hosts': devApi,
        '/api/history': devApi,
      },
    },
  }
})
