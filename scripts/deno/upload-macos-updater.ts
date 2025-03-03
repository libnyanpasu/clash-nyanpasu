import * as path from 'jsr:@std/path'
import { globby } from 'npm:globby'
import { consola } from './utils/logger.ts'

const WORKSPACE_ROOT = path.join(Deno.cwd(), '../..')
consola.info(`WORKSPACE_ROOT: ${WORKSPACE_ROOT}`)

const GITHUB_TOKEN = Deno.env.get('GITHUB_TOKEN') || Deno.env.get('GH_TOKEN')
const GITHUB_TAG = Deno.env.get('GITHUB_TAG')
const TARGET_ARCH = Deno.env.get('TARGET_ARCH') || Deno.build.arch

if (!GITHUB_TOKEN) {
  consola.fatal('GITHUB_TOKEN is not set')
  Deno.exit(1)
}

if (!GITHUB_TAG) {
  consola.fatal('GITHUB_TAG is not set')
  Deno.exit(1)
}

const BACKEND_BUILD_DIR = path.join(WORKSPACE_ROOT, 'backend/target')

const files = await globby(['**/*.tar.gz', '**/*.sig', '**/*.dmg'], {
  cwd: BACKEND_BUILD_DIR,
})

for (let file of files) {
  file = path.join(BACKEND_BUILD_DIR, file)
  const p = path.parse(file)
  consola.info(`Found file: ${p.base}`)
  if (p.base.endsWith('.app.tar.gz')) {
    const newName = p.name.split('.')[0] + `.${TARGET_ARCH}.app.tar.gz`
    const newPath = path.join(p.dir, newName)
    consola.info(`Renaming ${file} to ${newPath}`)
    await Deno.rename(file, newPath)
    file = newPath
  }
  consola.info(`Uploading ${file}...`)
  const cmd = new Deno.Command('gh', {
    args: ['release', 'upload', GITHUB_TAG, file, '--clobber'],
    stdout: 'piped',
    stderr: 'piped',
    env: {
      GH_TOKEN: GITHUB_TOKEN,
      GITHUB_TOKEN,
    },
  })

  const output = await cmd.output()
  if (output.code !== 0) {
    consola.error(output.stderr)
    consola.error(`Failed to upload ${file}`)
    Deno.exit(1)
  }
}

consola.success('Uploaded all files')
