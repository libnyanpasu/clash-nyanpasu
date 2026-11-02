import { mkdir, rm } from 'node:fs/promises'
import * as path from 'node:path'
import { isNotFound } from '../shared/errors.ts'
import { runArchivePublish } from './internet-archive-upload.ts'

async function main(): Promise<void> {
  const [manifest, server, report] = process.argv.slice(2)
  if (!manifest || !server || !report) {
    throw new Error(
      'Usage: archive:publish <manifest.json> <server-url> <report.json>',
    )
  }
  const reportPath = path.resolve(report)
  await mkdir(path.dirname(reportPath), { recursive: true })
  await rm(reportPath).catch((error) => {
    if (!isNotFound(error)) throw error
  })
  process.exitCode = await runArchivePublish([
    '--manifest',
    path.resolve(manifest),
    '--server',
    server,
    '--report',
    reportPath,
  ])
}

if (import.meta.main) await main()
