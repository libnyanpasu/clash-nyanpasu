import JavascriptRounded from '~icons/material-symbols/javascript-rounded'
import MergeRounded from '~icons/material-symbols/merge-rounded'
import LuaIcon from '~icons/mdi/language-lua'
import type { ComponentProps } from 'react'
import { ProfileType } from './consts'

const TRANSFORM_TYPE_ICONS = {
  [ProfileType.JavaScript]: JavascriptRounded,
  [ProfileType.Lua]: LuaIcon,
  [ProfileType.Merge]: MergeRounded,
} as const

/** The language mark of a transform profile, or nothing for any other type. */
export default function TransformTypeIcon({
  type,
  ...props
}: ComponentProps<'svg'> & { type: ProfileType }) {
  const Icon =
    type === ProfileType.JavaScript ||
    type === ProfileType.Lua ||
    type === ProfileType.Merge
      ? TRANSFORM_TYPE_ICONS[type]
      : null

  return Icon && <Icon aria-hidden {...props} />
}
