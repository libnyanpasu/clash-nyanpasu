import path from 'path'
import fs from 'fs-extra'
import { TAURI_APP_DIR } from './utils/env'
import { consola } from './utils/logger'

const TAURI_APP_CONF = path.join(TAURI_APP_DIR, 'tauri.conf.json')

const TAURI_PREVIEW_APP_CONF_PATH = path.join(
  TAURI_APP_DIR,
  'tauri.preview.conf.json',
)

const main = async () => {
  consola.debug('Read config...')

  const tauriAppConf = await fs.readJSON(TAURI_APP_CONF)

  tauriAppConf.build.devPath = tauriAppConf.build.distDir
  tauriAppConf.build.beforeDevCommand = tauriAppConf.build.beforeBuildCommand

  consola.debug('Write config...')

  await fs.writeJSON(TAURI_PREVIEW_APP_CONF_PATH, tauriAppConf, {
    spaces: 2,
  })
}

main()
