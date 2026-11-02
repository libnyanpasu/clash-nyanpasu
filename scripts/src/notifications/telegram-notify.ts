import { readFile } from 'node:fs/promises'
import * as path from 'node:path'
import { Bot } from 'grammy'
import { runCommand } from '../shared/command.ts'
import { consola } from '../shared/logger.ts'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'

interface ReleaseNotification {
  nightly: boolean
  tag: string
  gitShortHash: string
  workflowRunId?: string
}

export function buildReleaseMessage(notification: ReleaseNotification): string {
  const { nightly, tag, gitShortHash, workflowRunId } = notification
  const lines = nightly
    ? [
        `Clash Nyanpasu Nightly Build ${gitShortHash}`,
        '',
        '⚠️ Could be unstable, use at your own risk.',
      ]
    : [
        `Clash Nyanpasu ${tag} Released!`,
        '',
        'GitHub Release:',
        `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/${encodeURIComponent(
          tag,
        )}`,
      ]

  if (nightly && workflowRunId) {
    lines.push(
      '',
      'GitHub Actions:',
      `https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/${workflowRunId}`,
    )
  }
  lines.push(
    '',
    'Downloads:',
    `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/${encodeURIComponent(
      tag,
    )}`,
  )

  return lines.join('\n')
}

function requireEnv(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)

  return value
}

async function main(): Promise<void> {
  const nightly = process.argv.slice(2).includes('--nightly')
  const bot = new Bot(requireEnv('TELEGRAM_TOKEN'))
  const chatId = requireEnv(
    nightly ? 'TELEGRAM_ARCHIVE_CHANNEL' : 'TELEGRAM_RELEASE_CHANNEL',
  )
  const pkg = JSON.parse(
    await readFile(path.join(WORKSPACE_ROOT, 'package.json'), 'utf8'),
  )
  const tag =
    process.env.RELEASE_TAG || (nightly ? 'pre-release' : `v${pkg.version}`)
  let gitShortHash = ''
  if (nightly) {
    const result = await runCommand('git', {
      args: ['rev-parse', '--short', 'HEAD'],
      cwd: WORKSPACE_ROOT,
      stdout: 'piped',
    })
    if (!result.success) throw new Error('Failed to read build commit')
    gitShortHash = result.stdout.toString('utf8').trim()
  }

  await bot.api.sendMessage(
    chatId,
    buildReleaseMessage({
      nightly,
      tag,
      gitShortHash,
      workflowRunId: process.env.WORKFLOW_RUN_ID,
    }),
  )
  consola.success('Sent telegram notification')
}

if (import.meta.main) {
  main().catch((error) => {
    consola.fatal(error)
    process.exit(1)
  })
}
