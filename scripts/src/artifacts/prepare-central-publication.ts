import { appendFile } from 'node:fs/promises'
import { parseArgs } from 'node:util'
import { prepareCentralPublication } from './central-publication.ts'

async function main(): Promise<void> {
  const args = parseArgs({
    options: {
      channel: { type: 'string' },
      'artifacts-dir': { type: 'string' },
      'output-dir': { type: 'string' },
      commit: { type: 'string' },
      'run-id': { type: 'string' },
      attempt: { type: 'string' },
      tag: { type: 'string' },
      'item-prefix': { type: 'string' },
      'published-at': { type: 'string' },
    },
    allowPositionals: true,
    strict: false,
  }).values as {
    channel?: string
    'artifacts-dir'?: string
    'output-dir'?: string
    commit?: string
    'run-id'?: string
    attempt?: string
    tag?: string
    'item-prefix'?: string
    'published-at'?: string
  }
  const required = (value: string | undefined, name: string): string => {
    if (!value) throw new Error(`--${name} is required`)
    return value
  }
  const channel = required(args.channel, 'channel')
  if (channel !== 'nightly' && channel !== 'release') {
    throw new Error('--channel must be nightly or release')
  }
  const context = await prepareCentralPublication(
    required(args['artifacts-dir'], 'artifacts-dir'),
    required(args['output-dir'], 'output-dir'),
    {
      channel,
      tag: channel === 'release' ? required(args.tag, 'tag') : null,
      commit: required(args.commit, 'commit'),
      runId: required(args['run-id'], 'run-id'),
      attempt: required(args.attempt, 'attempt'),
      itemPrefix: args['item-prefix'] ?? 'sourceforge-only',
      publishedAt: args['published-at'],
    },
  )
  if (process.env['GITHUB_OUTPUT']) {
    await appendFile(
      process.env['GITHUB_OUTPUT']!,
      `published_at=${context.publishedAt}\n`,
    )
  }
  console.log(
    `Prepared ${context.targets.length} target inventories at ${context.publishedAt}`,
  )
}

if (import.meta.main) await main()
