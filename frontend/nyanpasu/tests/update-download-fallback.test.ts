import { expect, test, vi } from 'vitest'
import { downloadUpdateWithFallback } from '../src/utils/update-download-fallback.ts'

const fakeUpdate = (
  name: string,
  attempts: string[],
  result: 'success' | 'download failure' | 'signature failure',
) => ({
  download: vi.fn(async () => {
    attempts.push(name)
    if (result !== 'success') throw new Error(result)
  }),
  install: vi.fn(async () => {}),
})

test('tries sources in priority order and returns the first successful package', async () => {
  const attempts: string[] = []
  const github = fakeUpdate('github', attempts, 'success')

  const downloaded = await downloadUpdateWithFallback([
    {
      source: 'nyanpasu',
      update: fakeUpdate('nyanpasu', attempts, 'download failure'),
    },
    { source: 'github', update: github },
  ])

  expect(downloaded).toBe(github)
  expect(attempts).toEqual(['nyanpasu', 'github'])
})

test('tries a single configured source only once', async () => {
  const attempts: string[] = []
  const github = fakeUpdate('github', attempts, 'success')

  await expect(
    downloadUpdateWithFallback([{ source: 'github', update: github }]),
  ).resolves.toBe(github)
  expect(attempts).toEqual(['github'])
})

test('falls back after signature failure and never installs a failed download', async () => {
  const attempts: string[] = []
  const nyanpasu = fakeUpdate('nyanpasu', attempts, 'signature failure')
  const github = fakeUpdate('github', attempts, 'success')

  const downloaded = await downloadUpdateWithFallback([
    { source: 'nyanpasu', update: nyanpasu },
    { source: 'github', update: github },
  ])

  expect(downloaded).toBe(github)
  expect(attempts).toEqual(['nyanpasu', 'github'])
  await downloaded.install()
  expect(nyanpasu.install).not.toHaveBeenCalled()
  expect(github.install).toHaveBeenCalledOnce()
})

test('stops after the first successful source', async () => {
  const attempts: string[] = []
  const nyanpasu = fakeUpdate('nyanpasu', attempts, 'success')
  const github = fakeUpdate('github', attempts, 'success')

  await expect(
    downloadUpdateWithFallback([
      { source: 'nyanpasu', update: nyanpasu },
      { source: 'github', update: github },
    ]),
  ).resolves.toBe(nyanpasu)
  expect(attempts).toEqual(['nyanpasu'])
  expect(github.download).not.toHaveBeenCalled()
})

test('reports every source error when all downloads fail', async () => {
  const attempts: string[] = []

  await expect(
    downloadUpdateWithFallback([
      {
        source: 'nyanpasu',
        update: fakeUpdate('nyanpasu', attempts, 'download failure'),
      },
      {
        source: 'github',
        update: fakeUpdate('github', attempts, 'signature failure'),
      },
    ]),
  ).rejects.toThrow(
    /nyanpasu: Error: download failure[\s\S]*github: Error: signature failure/,
  )
  expect(attempts).toEqual(['nyanpasu', 'github'])
})
