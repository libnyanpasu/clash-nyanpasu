import { ClashManifest } from 'types'
import versionManifest from '../../manifest/version.json'

export const CLASH_META_MANIFEST: ClashManifest = {
  URL_PREFIX: `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}`,
  VERSION: versionManifest.latest.mihomo,
  ARCH_MAPPING: versionManifest.arch_template.mihomo,
}

export const CLASH_META_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt',
  URL_PREFIX:
    'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha',
  VERSION: versionManifest.latest.mihomo_alpha,
  ARCH_MAPPING: versionManifest.arch_template.mihomo_alpha,
}
