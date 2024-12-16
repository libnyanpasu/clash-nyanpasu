import { ClashManifest } from 'types'
import versionManifest from '../../manifest/version.json'

export const CLASH_MANIFEST: ClashManifest = {
  URL_PREFIX: 'https://github.com/Dreamacro/clash/releases/download/premium/',
  LATEST_DATE: '2023.08.17',
  STORAGE_PREFIX: 'https://release.dreamacro.workers.dev/',
  BACKUP_URL_PREFIX:
    'https://github.com/zhongfly/Clash-premium-backup/releases/download/',
  BACKUP_LATEST_DATE: versionManifest.latest.clash_premium,
  VERSION: versionManifest.latest.clash_premium,
  ARCH_MAPPING: versionManifest.arch_template.clash_premium,
}
