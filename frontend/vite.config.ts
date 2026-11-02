import path from 'node:path'
import fs from 'node:fs'
import process from 'node:process'
import { defineConfig, loadEnv } from 'vite'
import vue from '@vitejs/plugin-vue'
import Components from 'unplugin-vue-components/vite'
import AutoImport from 'unplugin-auto-import/vite'

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd())
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
        '/json': env.VITE_DEV_API,
        '/api/history': env.VITE_DEV_API,
      },
    },
  }
})
