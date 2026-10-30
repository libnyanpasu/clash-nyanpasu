import { writeFile } from 'node:fs/promises'
import { parseArgs } from 'node:util'
import {
  createIaItemIdentifier,
  preparePublicationManifest,
} from './publication-manifest.ts'

async function main(): Promise<void> {
  const args = parseArgs({
    options: {
      channel: { type: 'string' },
      target: { type: 'string' },
      'build-id': { type: 'string' },
      'item-prefix': { type: 'string' },
      commit: { type: 'string' },
      tag: { type: 'string' },
      folder: { type: 'string' },
      output: { type: 'string' },
      pattern: { type: 'string', short: 'p', multiple: true },
    },
    allowPositionals: true,
    strict: false,
  }).values as {
    channel?: string
    target?: string
    'build-id'?: string
    'item-prefix'?: string
    commit?: string
    tag?: string
    folder?: string
    output?: string
    pattern?: string[]
  }
  const requireValue = (value: string | undefined, name: string): string => {
    if (!value) throw new Error(`--${name} is required`)
    return value
  }
  const channel = requireValue(args.channel, 'channel') as 'release' | 'nightly'
  const target = requireValue(args.target, 'target')
  const commit = requireValue(args.commit, 'commit')
  const itemPrefix = args['item-prefix'] || 'sourceforge-only'
  const runId = process.env['GITHUB_RUN_ID'] ?? '0'
  const attempt = process.env['GITHUB_RUN_ATTEMPT'] ?? '1'
  const itemIdentifier = createIaItemIdentifier(
    itemPrefix,
    channel,
    runId,
    attempt,
    target,
    commit,
  )
  const manifest = await preparePublicationManifest({
    channel,
    target,
    buildId: requireValue(args['build-id'], 'build-id'),
    itemPrefix,
    commit,
    tag: channel === 'release' ? requireValue(args.tag, 'tag') : null,
    folderPath: requireValue(args.folder, 'folder'),
    itemIdentifier,
    paths: ((args.pattern ?? []) as string[]).filter(Boolean),
  })
  const output = requireValue(args.output, 'output')
  await writeFile(output, `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(
    `Prepared ${manifest.artifacts.length} artifacts for ${target}: ${output}`,
  )
}

if (import.meta.main) await main()
