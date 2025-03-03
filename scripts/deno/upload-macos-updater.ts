import * as path from 'jsr:@std/path'
import { globby } from 'npm:globby'
import { consola } from './utils/logger.ts'

const WORKSPACE_ROOT = path.join(Deno.cwd(), '../..')
consola.info(`WORKSPACE_ROOT: ${WORKSPACE_ROOT}`)

const GITHUB_TOKEN = Deno.env.get('GITHUB_TOKEN') || Deno.env.get('GH_TOKEN')
const GITHUB_TAG = Deno.env.get('GITHUB_TAG')

if (!GITHUB_TOKEN) {
  consola.fatal('GITHUB_TOKEN is not set')
  Deno.exit(1)
}

if (!GITHUB_TAG) {
  consola.fatal('GITHUB_TAG is not set')
  Deno.exit(1)
}

const files = await globby(
  [
    'target/backend/**/*.tar.gz',
    'target/backend/**/*.sig',
    'target/backend/**/*.dmg',
  ],
  {
    cwd: WORKSPACE_ROOT,
  },
)

for (let file of files) {
  const p = path.parse(file)
  if (p.name.endsWith('.app.tar.gz')) {
    const arch = Deno.build.arch
    const newName = name.split('.')[0] + `.${arch}.app.tar.gz`
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
