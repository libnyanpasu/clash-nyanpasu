import z from 'zod'
import { definitionOf as importDefinitionOf } from '@/components/profile-import/import-kind'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import { ProfileTemplate } from '@nyanpasu/query'
import {
  type ProfileDefinition_Deserialize,
  type ProfileImportKind,
  type ProfileSource_Deserialize,
} from '@nyanpasu/rpc/types'
import { ProfileType } from './consts'
import { subscriptionUrlSchema } from './subscription-url-schema'

export const CONFIG_KINDS = ['file', 'composition'] as const

export const TRANSFORM_KINDS = [
  ProfileType.JavaScript,
  ProfileType.Lua,
  ProfileType.Merge,
] as const

export type CreateKind =
  (typeof CONFIG_KINDS)[number] | (typeof TRANSFORM_KINDS)[number]

export type CreateSource = 'remote' | 'local' | 'external'

export const formSchema = z
  .object({
    kind: z.enum([...CONFIG_KINDS, ...TRANSFORM_KINDS]),
    source: z.enum(['remote', 'local', 'external']),
    name: z.string(),
    desc: z.string(),
    url: z.string(),
    option: z.object({
      user_agent: z.string(),
      with_proxy: z.boolean(),
      self_proxy: z.boolean(),
      update_interval: z
        .number()
        .min(1, {
          message: m.profile_form_option_update_interval_min_error(),
        })
        .nullable(),
    }),
    fileName: z.string().nullable(),
    fileContent: z.string().nullable(),
    externalPath: z.string(),
    externalMode: z.enum(['mirror', 'symlink']),
    members: z.array(z.string()),
  })
  .superRefine((data, ctx) => {
    if (data.kind === 'composition') {
      if (data.members.length < 2) {
        ctx.addIssue({
          code: 'custom',
          path: ['members'],
          message: m.profile_composition_min_members_hint(),
        })
      }
      return
    }

    if (data.source === 'remote') {
      const url = subscriptionUrlSchema.safeParse(data.url)
      if (!url.success) {
        ctx.addIssue({
          code: 'custom',
          path: ['url'],
          message: url.error.issues[0].message,
        })
      }
    }

    if (data.source === 'external' && !data.externalPath) {
      ctx.addIssue({
        code: 'custom',
        path: ['externalPath'],
        message: m.profile_external_path_required(),
      })
    }
  })

export type FormValues = z.infer<typeof formSchema>

export const getDefaultValues = (
  kind: CreateKind,
  source: CreateSource,
): FormValues => ({
  kind,
  source,
  name: '',
  desc: '',
  url: '',
  option: {
    user_agent: '',
    with_proxy: false,
    self_proxy: false,
    update_interval: null,
  },
  fileName: null,
  fileContent: null,
  externalPath: '',
  externalMode: 'mirror',
  members: [],
})

export const KIND_LABELS = {
  file: () => m.profile_kind_file(),
  composition: () => m.profile_kind_composition(),
  [ProfileType.JavaScript]: () => 'JavaScript',
  [ProfileType.Lua]: () => 'Lua',
  [ProfileType.Merge]: () => 'YAML',
} satisfies Record<CreateKind, () => string>

export const ACCEPT_EXTENSIONS = {
  file: ['.yaml', '.yml'],
  composition: [],
  [ProfileType.JavaScript]: ['.js'],
  [ProfileType.Lua]: ['.lua'],
  [ProfileType.Merge]: ['.yaml', '.yml'],
} satisfies Record<CreateKind, string[]>

const TEMPLATES = {
  file: ProfileTemplate.profile,
  composition: null,
  [ProfileType.JavaScript]: ProfileTemplate.javascript,
  [ProfileType.Lua]: ProfileTemplate.luascript,
  [ProfileType.Merge]: ProfileTemplate.merge,
} satisfies Record<CreateKind, string | null>

export const templateOf = (kind: CreateKind) => TEMPLATES[kind]

const IMPORT_KINDS = {
  file: 'config',
  [ProfileType.JavaScript]: 'javascript',
  [ProfileType.Lua]: 'lua',
  [ProfileType.Merge]: 'overlay',
} satisfies Record<Exclude<CreateKind, 'composition'>, ProfileImportKind>

export const definitionOf = (
  kind: Exclude<CreateKind, 'composition'>,
  source: ProfileSource_Deserialize,
): ProfileDefinition_Deserialize =>
  importDefinitionOf(IMPORT_KINDS[kind], source)

const baseName = (path: string) =>
  path
    .split(/[\\/]/)
    .at(-1)
    ?.replace(/\.[^.]+$/, '') ?? ''

/** The user's name, else the picked file's stem, else a timestamped label. */
export const fallbackName = (values: FormValues) => {
  const picked =
    values.source === 'external' ? values.externalPath : values.fileName
  return (
    values.name.trim() ||
    (picked && baseName(picked)) ||
    `${KIND_LABELS[values.kind]()} - ${formatDate(new Date())}`
  )
}
