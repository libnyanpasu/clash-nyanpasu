import { useLockFn } from 'ahooks'
import { Reorder } from 'framer-motion'
import { useAtomValue } from 'jotai'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Add } from '@mui/icons-material'
import { ListItemButton } from '@mui/material'
import { ProfileQueryResultItem, useProfile } from '@nyanpasu/interface'
import { alpha } from '@nyanpasu/ui'
import { ClashProfile, ClashProfileBuilder, filterProfiles } from '../utils'
import ChainItem from './chain-item'
import { atomChainsSelected, atomGlobalChainCurrent } from './store'

export interface SideChainProps {
  onChainEdit: (item?: ProfileQueryResultItem) => void | Promise<void>
}

export const SideChain = ({ onChainEdit }: SideChainProps) => {
  const { t } = useTranslation()

  const isGlobalChainCurrent = useAtomValue(atomGlobalChainCurrent)

  const currentProfileUid = useAtomValue(atomChainsSelected)

  const { query, upsert, patch, sort } = useProfile()

  const profiles = query.data

  const { clash, chain } = filterProfiles(profiles?.items)

  const currentProfile = useMemo(() => {
    return clash?.find((item) => item.uid === currentProfileUid) as ClashProfile
  }, [clash, currentProfileUid])

  // Filter chains to show only relevant ones based on global/local context
  const filteredChains = useMemo(() => {
    if (isGlobalChainCurrent) {
      // When in global chain mode, show all chain profiles
      return chain || []
    } else {
      // In local chain mode, show all chain profiles so user can add them to the current profile's chain
      // This is the expected behavior for local chains - users should see all available chains to choose from
      return chain || []
    }
  }, [chain, isGlobalChainCurrent])

  const handleChainClick = useLockFn(async (uid: string) => {
    const chains = isGlobalChainCurrent
      ? (profiles?.chain ?? [])
      : (currentProfile?.chain ?? [])

    const updatedChains = chains.includes(uid)
      ? chains.filter((chain) => chain !== uid)
      : [...chains, uid]

    try {
      if (isGlobalChainCurrent) {
        await upsert.mutateAsync({ chain: updatedChains })
      } else {
        if (!currentProfile?.uid) {
          return
        }
        await patch.mutateAsync({
          uid: currentProfile.uid,
          profile: {
            ...(currentProfile as ClashProfileBuilder),
            chain: updatedChains,
          },
        })
      }
    } catch (e) {
      message(`Apply error: ${formatError(e)}`, {
        kind: 'error',
        title: t('Error'),
      })
    }
  })

  const reorderValues = useMemo(
    () => filteredChains?.map((item) => item.uid) || [],
    [filteredChains],
  )

  return (
    <div className="h-full overflow-auto !pr-2 !pl-2">
      <Reorder.Group
        axis="y"
        values={reorderValues}
        onReorder={(values) => {
          const profileUids = clash?.map((item) => item.uid) || []
          sort.mutate([...profileUids, ...values])
        }}
        layoutScroll
        style={{ overflowY: 'scroll' }}
      >
        {filteredChains?.map((item, index) => {
          const selected = isGlobalChainCurrent
            ? profiles?.chain?.includes(item.uid)
            : currentProfile?.chain?.includes(item.uid)

          // Check if chain is used in global context
          const usedInGlobal = profiles?.chain?.includes(item.uid)

          // Check if chain is used in current profile context
          const usedInCurrentProfile = currentProfile?.chain?.includes(item.uid)

          return (
            <ChainItem
              key={index}
              item={item}
              selected={selected}
              context={{
                scope: usedInGlobal
                  ? 'global'
                  : usedInCurrentProfile
                    ? 'scoped'
                    : 'global',
              }}
              onClick={async () => await handleChainClick(item.uid)}
              onChainEdit={() => onChainEdit(item)}
            />
          )
        })}
      </Reorder.Group>

      <ListItemButton
        className="!mt-2 !mb-2 flex justify-center gap-2"
        sx={(theme) => ({
          backgroundColor: alpha(theme.vars.palette.secondary.main, 0.1),
          borderRadius: 4,
        })}
        onClick={() => onChainEdit()}
      >
        <Add color="primary" />

        <div className="py-1">{t('New Chain')}</div>
      </ListItemButton>
    </div>
  )
}
