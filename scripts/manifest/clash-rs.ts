import { ClashManifest } from 'types'
import versionManifest from '../../manifest/version.json'

export const CLASH_RS_MANIFEST: ClashManifest = {
  URL_PREFIX: 'https://github.com/Watfaq/clash-rs/releases/download/',
  VERSION: versionManifest.latest.clash_rs,
  ARCH_MAPPING: versionManifest.arch_template.clash_rs,
}

export const CLASH_RS_ALPHA_MANIFEST: ClashManifest = {
  VERSION_URL:
    'https://github.com/Watfaq/clash-rs/releases/download/latest/version.txt',
  URL_PREFIX: 'https://github.com/Watfaq/clash-rs/releases/download/latest',
  VERSION: versionManifest.latest.clash_rs_alpha,
  ARCH_MAPPING: versionManifest.arch_template.clash_rs_alpha,
}
