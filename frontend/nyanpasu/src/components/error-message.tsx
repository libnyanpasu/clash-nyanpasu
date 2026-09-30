import { formatError } from '@/utils'
import { profileMessageParts } from '@/utils/profile-label'
import { StepLabel, useOpenProfile, useProfileLookup } from './profile-label'

export function ErrorMessage({ error }: { error: unknown }) {
  const profiles = useProfileLookup()
  const openProfile = useOpenProfile(profiles)
  return (
    <span className="[overflow-wrap:anywhere] whitespace-pre-wrap">
      <StepLabel
        parts={profileMessageParts((label) => formatError(error, label))}
        profiles={profiles}
        onOpenProfile={openProfile}
      />
    </span>
  )
}
