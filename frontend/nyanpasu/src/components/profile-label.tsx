import { useMemo } from 'react'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { profileTypeOf } from '@/pages/(main)/main/profiles/$type/_modules/utils'
import type { LabelPart, ProfileLookup } from '@/utils/profile-label'
import { useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { useNavigate } from '@tanstack/react-router'

export { stepParts, stepKind, stepSubject } from '@/utils/profile-label'
export type { LabelPart, ProfileLookup } from '@/utils/profile-label'

const profileTagClassName = cn(
  'bg-tertiary-container text-on-tertiary-container',
  'inline-block max-w-full rounded-md px-1.5 py-0.5 text-xs [overflow-wrap:anywhere]',
)

/** A profile that no longer exists falls back to its raw id. */
export function profileName(profiles: ProfileLookup, id: string): string {
  return profiles.get(id)?.name ?? id
}

export function partsText(parts: LabelPart[], profiles: ProfileLookup) {
  return parts
    .map((part) =>
      typeof part === 'string' ? part : profileName(profiles, part.profileId),
    )
    .join('')
}

export function useProfileLookup(): ProfileLookup {
  const { query } = useProfile()
  return useMemo(
    () => new Map(query.data?.items?.map((item) => [item.uid, item]) ?? []),
    [query.data?.items],
  )
}

/** Opens the detail page of a profile; unknown ids are ignored. */
export function useOpenProfile(profiles: ProfileLookup) {
  const navigate = useNavigate()
  return (id: string) => {
    const profile = profiles.get(id)
    if (!profile) return
    navigate({
      to: '/main/profiles/$type/detail/$uid',
      params: { type: profileTypeOf(profile), uid: profile.uid },
    })
  }
}

export function StepLabel({
  parts,
  profiles,
  onOpenProfile,
}: {
  parts: LabelPart[]
  profiles: ProfileLookup
  onOpenProfile: (id: string) => void
}) {
  return parts.map((part, index) =>
    typeof part === 'string' ? (
      part
    ) : (
      <Tooltip key={index}>
        <TooltipTrigger asChild>
          <span
            className={profileTagClassName}
            onDoubleClick={() => onOpenProfile(part.profileId)}
          >
            {profileName(profiles, part.profileId)}
          </span>
        </TooltipTrigger>
        <TooltipContent className="font-mono">{part.profileId}</TooltipContent>
      </Tooltip>
    ),
  )
}
