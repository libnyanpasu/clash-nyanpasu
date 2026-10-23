import { useMemo } from 'react'
import { m } from '@/paraglide/messages'
import { scopedTransformsOf, useProfileMutations } from '@nyanpasu/query'
import { type ProfileDefinition_Deserialize } from '@nyanpasu/rpc/types'
import { TransformChainList } from '../../../_modules/transform-chain-list'
import { type ConfigProfile } from '../../../_modules/utils'
import { DetailSection } from './detail-section'

/** The transform chain that applies only to one config item. */
export default function TransformChainSection({
  profile,
}: {
  profile: ConfigProfile
}) {
  const { replaceDefinition } = useProfileMutations()

  // The edited config item's own scoped transforms (File or Composition).
  const transforms = useMemo(() => scopedTransformsOf(profile), [profile])

  return (
    <DetailSection title={m.profile_chain_editor_title()}>
      {/* Keyed per profile so a draft cannot leak into another config whose
          baseline happens to be identical. */}
      <TransformChainList
        key={profile.uid}
        taskKey={`update-chain-${profile.uid}`}
        traceStepOf={(uid, index) => `scoped:${profile.uid}:${uid}:${index}`}
        transforms={transforms}
        onApply={async (next) => {
          // Rebuild the config definition with the reordered scoped
          // transforms, preserving every other field, and replace it
          // atomically.
          const definition: ProfileDefinition_Deserialize = {
            type: 'config',
            config: { ...profile.config, transforms: next },
          }

          await replaceDefinition.mutateAsync({
            uid: profile.uid,
            definition,
          })
        }}
      />
    </DetailSection>
  )
}
