import { rmSync } from 'node:fs'
import { dirname } from 'node:path'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// Only generated Theme Light chunks are removed. Other embedded server assets
// and Jinja templates under web/ remain untouched.
const scriptDirectory = dirname(fileURLToPath(import.meta.url))
rmSync(resolve(scriptDirectory, '../../web/theme-light-assets'), {
  force: true,
  recursive: true,
})
