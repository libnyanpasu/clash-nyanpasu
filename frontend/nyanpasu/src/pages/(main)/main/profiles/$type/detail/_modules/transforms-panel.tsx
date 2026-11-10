import { useMemo } from 'react'
import { m } from '@/paraglide/messages'
import { scopedTransformsOf, useProfileMutations } from '@nyanpasu/query'
import { type ProfileDefinition_Deserialize } from '@nyanpasu/rpc/types'
import { useBlocker } from '@tanstack/react-router'
import LeaveDialog from '../../../_modules/leave-dialog'
import {
  TransformChain,
  TransformChainBar,
  TransformChainHeader,
} from '../../../_modules/transform-chain-list'
import { TransformLibrary } from '../../../_modules/transform-library'
import { useTransformChainDraft } from '../../../_modules/use-transform-chain-draft'
import { type ConfigProfile } from '../../../_modules/utils'
import AddTransformDrawer from './add-transform-drawer'
import { DetailPanel, type DetailPanelLayout } from './detail-panel'

/**
 * The transform chain that applies only to one config item, edited in the
 * detail page's panel. The draft lives here, so leaving the panel with
 * unapplied changes asks first. Render it keyed by the profile so a draft
 * cannot leak into another config whose baseline happens to be identical.
 */
export default function TransformsPanel({
  profile,
  layout,
  onClose,
}: {
  profile: ConfigProfile
  layout: DetailPanelLayout
  onClose: () => void
}) {
  const { replaceDefinition } = useProfileMutations()

  // The edited config item's own scoped transforms (File or Composition).
  const transforms = useMemo(() => scopedTransformsOf(profile), [profile])

  const chain = useTransformChainDraft({
    transforms,
    taskKey: `update-chain-${profile.uid}`,
    onApply: async (next) => {
      // Rebuild the config definition with the reordered scoped transforms,
      // preserving every other field, and replace it atomically.
      const definition: ProfileDefinition_Deserialize = {
        type: 'config',
        config: { ...profile.config, transforms: next },
      }

      await replaceDefinition.mutateAsync({ uid: profile.uid, definition })
    },
  })

  const blocker = useBlocker({
    shouldBlockFn: () => true,
    disabled: !chain.isChanged,
    enableBeforeUnload: chain.isChanged,
    withResolver: true,
  })

  return (
    <>
      <DetailPanel
        title={m.profile_chain_editor_title()}
        profileName={profile.name}
        layout={layout}
        onClose={onClose}
      >
        {/* The library sits below the chain where there is room for it, and
            opens in a drawer where the panel takes the whole page. */}
        <TransformChain
          chain={chain}
          traceStepOf={(uid, index) => `scoped:${profile.uid}:${uid}:${index}`}
          header={
            layout === 'wide' ? (
              <TransformChainHeader chain={chain} />
            ) : (
              <div className="flex items-center justify-between gap-2">
                <TransformChainHeader chain={chain} />

                <AddTransformDrawer chain={chain} />
              </div>
            )
          }
          footer={
            layout === 'wide' ? (
              <>
                <TransformLibrary chain={chain} />

                <TransformChainBar chain={chain} />
              </>
            ) : (
              <TransformChainBar chain={chain} />
            )
          }
        />
      </DetailPanel>

      <LeaveDialog
        open={blocker.status === 'blocked'}
        description={m.profile_chain_editor_leave_description()}
        onStay={() => blocker.reset?.()}
        onLeave={() => blocker.proceed?.()}
      />
    </>
  )
}
