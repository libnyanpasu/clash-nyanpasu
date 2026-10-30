import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { runCommand } from '../shared/command.ts'
import {
  collectUpdaterPlatforms,
  type ReleaseAsset,
} from '../updater/updater-platforms.ts'

const tauriCli = createRequire(import.meta.url).resolve(
  '@tauri-apps/cli/tauri.js',
)

test('nightly updater signatures bind all installer variants to the release version', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'nyanpasu-sign-test-'))
  try {
    const keyPath = path.join(directory, 'test.key')
    const generated = await runCommand('node', {
      args: [
        tauriCli,
        'signer',
        'generate',
        '--ci',
        '--password',
        '',
        '--write-keys',
        keyPath,
      ],
      stdout: 'piped',
      stderr: 'piped',
    })
    assert.deepStrictEqual(generated.success, true)
    const env = {
      TAURI_SIGNING_PRIVATE_KEY: await readFile(keyPath, 'utf8'),
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '',
    }
    const version = '2.0.0-alpha+83fbe50'
    await writeFile(
      path.join(directory, 'package.json'),
      JSON.stringify({ version }),
    )
    const bundleDir = path.join(directory, 'backend/target/release/bundle')
    await mkdir(bundleDir, { recursive: true })
    const archives = [
      'Clash.Nyanpasu_2.0.0_x64-setup.nsis.zip',
      'Clash.Nyanpasu_2.0.0_fixed-webview-x64-setup.nsis.zip',
      'Clash.Nyanpasu_2.0.0_arm64-setup.nsis.zip',
      'Clash.Nyanpasu_2.0.0_fixed-webview-arm64-setup.nsis.zip',
      'Clash.Nyanpasu_2.0.0+alpha.83fbe50_amd64.AppImage.tar.gz',
    ]
    const assets: ReleaseAsset[] = []
    for (const name of archives) {
      const archive = path.join(bundleDir, name)
      await writeFile(archive, `updater fixture ${name}`)
      const signed = await runCommand('node', {
        args: [tauriCli, 'signer', 'sign', '--app-version', '2.0.0', archive],
        env,
        stdout: 'null',
        stderr: 'piped',
      })
      assert.deepStrictEqual(signed.success, true)
      assets.push(
        { name, browser_download_url: archive },
        { name: `${name}.sig`, browser_download_url: `${archive}.sig` },
      )
    }
    await assert.rejects(
      () =>
        collectUpdaterPlatforms(
          assets,
          (file: string) => readFile(file, 'utf8'),
          version,
        ),
      { message: /Signed version mismatch/ },
    )
    const result = await runCommand(process.execPath, {
      args: [
        fileURLToPath(new URL('sign-nightly-updater.ts', import.meta.url)),
      ],
      cwd: directory,
      env,
      stdout: 'piped',
      stderr: 'piped',
    })
    assert.deepStrictEqual(result.success, true, result.stderr.toString('utf8'))
    const platforms = await collectUpdaterPlatforms(
      assets,
      (file: string) => readFile(file, 'utf8'),
      `v${version}`,
    )
    assert.deepStrictEqual(Object.keys(platforms).length, 7)
    for (const { signature } of Object.values(platforms)) {
      assert.deepStrictEqual(
        atob(signature).split('\n')[2].endsWith(`\tversion:${version}`),
        true,
      )
    }
  } finally {
    await rm(directory, { recursive: true })
  }
})
