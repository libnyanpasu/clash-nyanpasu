import { m } from '@/paraglide/messages'
import type {
  ImportSignal,
  ProfileDefinition_Deserialize,
  ProfileImportKind,
  ProfileSource_Deserialize,
  TransformKind,
} from '@nyanpasu/rpc/types'

export const IMPORT_KINDS = [
  'config',
  'overlay',
  'javascript',
  'lua',
] as const satisfies readonly ProfileImportKind[]

export const IMPORT_KIND_LABELS = {
  config: () => m.profile_import_url_kind_config(),
  overlay: () => m.profile_import_url_kind_overlay(),
  javascript: () => 'JavaScript',
  lua: () => 'Lua',
} satisfies Record<ProfileImportKind, () => string>

export const IMPORT_SIGNAL_LABELS = {
  subscription_quota: () => m.profile_import_url_signal_subscription_quota(),
  profile_title: () => m.profile_import_url_signal_profile_title(),
  update_interval: () => m.profile_import_url_signal_update_interval(),
  file_extension: () => m.profile_import_url_signal_file_extension(),
  content_type: () => m.profile_import_url_signal_content_type(),
  proxies_key: () => m.profile_import_url_signal_proxies_key(),
  overlay_directives: () => m.profile_import_url_signal_overlay_directives(),
  script_syntax: () => m.profile_import_url_signal_script_syntax(),
} satisfies Record<ImportSignal, () => string>

/** `null` for a config, the matching transform kind otherwise. */
export const transformOf = (kind: ProfileImportKind): TransformKind | null => {
  switch (kind) {
    case 'config':
      return null
    case 'overlay':
      return { type: 'overlay' }
    case 'javascript':
    case 'lua':
      return { type: 'script', runtime: kind }
  }
}

export const definitionOf = (
  kind: ProfileImportKind,
  source: ProfileSource_Deserialize,
): ProfileDefinition_Deserialize => {
  const transform = transformOf(kind)
  if (!transform) {
    return { type: 'config', config: { type: 'file', source, transforms: [] } }
  }
  if (transform.type === 'overlay') {
    return { type: 'transform', transform: { type: 'overlay', source } }
  }
  return {
    type: 'transform',
    transform: { type: 'script', source, runtime: transform.runtime },
  }
}
