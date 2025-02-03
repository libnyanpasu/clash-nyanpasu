import MdiTextBoxCheckOutline from '~icons/mdi/text-box-check-outline'
import { useLockFn } from 'ahooks'
import { AnimatePresence, motion } from 'framer-motion'
import { useAtom } from 'jotai'
import { useMemo, useState, useTransition } from 'react'
import { useTranslation } from 'react-i18next'
import { useWindowSize } from 'react-use'
import { z } from 'zod'
import {
  atomChainsSelected,
  atomGlobalChainCurrent,
} from '@/components/profiles/modules/store'
import NewProfileButton from '@/components/profiles/new-profile-button'
import {
  AddProfileContext,
  AddProfileContextValue,
} from '@/components/profiles/profile-dialog'
import ProfileItem from '@/components/profiles/profile-item'
import ProfileSide from '@/components/profiles/profile-side'
import { GlobalUpdatePendingContext } from '@/components/profiles/provider'
import { QuickImport } from '@/components/profiles/quick-import'
import RuntimeConfigDiffDialog from '@/components/profiles/runtime-config-diff-dialog'
import { ClashProfile, filterProfiles } from '@/components/profiles/utils'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Public, Update } from '@mui/icons-material'
import { Badge, Button, CircularProgress, IconButton } from '@mui/material'
import Grid from '@mui/material/Grid2'
import {
  RemoteProfileOptionsBuilder,
  useClash,
  useProfile,
  type RemoteProfile,
} from '@nyanpasu/interface'
import { FloatingButton, SidePage } from '@nyanpasu/ui'
import { createFileRoute, useLocation } from '@tanstack/react-router'
import { zodSearchValidator } from '@tanstack/router-zod-adapter'

const profileSearchParams = z.object({
  subscribeName: z.string().optional(),
  subscribeUrl: z.string().url().optional(),
  subscribeDesc: z.string().optional(),
})

export const Route = createFileRoute('/profiles')({
  validateSearch: zodSearchValidator(profileSearchParams),
  component: ProfilePage,
})

function ProfilePage() {
  const { t } = useTranslation()

  const { getRuntimeLogs } = useClash()

  const { query, update } = useProfile()

  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items)
  }, [query.data?.items])

  const maxLogLevelTriggered = useMemo(() => {
    const currentProfileChains =
      (
        query.data?.items?.find(
          // TODO: 支持多 Profile
          (item) => query.data?.current?.[0] === item.uid,
          // TODO: fix any type
        ) as any
      )?.chain || []
    return Object.entries(getRuntimeLogs.data || {}).reduce(
      (acc, [key, value]) => {
        const accKey = currentProfileChains.includes(key) ? 'current' : 'global'
        if (acc[accKey] === 'error') {
          return acc
        }
        for (const log of value) {
          switch (log[0]) {
            case 'error':
              return { ...acc, [accKey]: 'error' }
            case 'warn':
              acc = { ...acc, [accKey]: 'warn' }
              break
            case 'info':
              if (acc[accKey] !== 'warn') {
                acc = { ...acc, [accKey]: 'info' }
              }
              break
          }
        }
        return acc
      },
      {} as {
        global: undefined | 'info' | 'error' | 'warn'
        current: undefined | 'info' | 'error' | 'warn'
      },
    )
  }, [query.data, getRuntimeLogs.data])

  const [globalChain, setGlobalChain] = useAtom(atomGlobalChainCurrent)

  const [chainsSelected, setChainsSelected] = useAtom(atomChainsSelected)

  const handleGlobalChainClick = () => {
    setChainsSelected(undefined)
    setGlobalChain(!globalChain)
  }

  const onClickChains = (profile: ClashProfile) => {
    setGlobalChain(false)

    if (chainsSelected === profile.uid) {
      setChainsSelected(undefined)
    } else {
      setChainsSelected(profile.uid)
    }
  }

  const handleSideClose = () => {
    setChainsSelected(undefined)
    setGlobalChain(false)
  }

  const [runtimeConfigViewerOpen, setRuntimeConfigViewerOpen] = useState(false)
  const location = useLocation()
  const addProfileCtxValue = useMemo(() => {
    if (!location.search || !location.search.subscribeUrl) {
      return null
    }
    return {
      name: location.search.subscribeName!,
      desc: location.search.subscribeDesc!,
      url: location.search.subscribeUrl,
    } satisfies AddProfileContextValue
  }, [location.search])

  const hasSide = globalChain || chainsSelected

  const { width } = useWindowSize()

  const [globalUpdatePending, startGlobalUpdate] = useTransition()
  const handleGlobalProfileUpdate = useLockFn(async () => {
    startGlobalUpdate(async () => {
      const remoteProfiles =
        (profiles.clash?.filter(
          (item) => item.type === 'remote',
        ) as RemoteProfile[]) || []

      const updates: Array<Promise<void>> = []

      for (const profile of remoteProfiles) {
        const option = {
          with_proxy: false,
          self_proxy: false,
          update_interval: 0,
          user_agent: profile.option?.user_agent ?? null,
          ...profile.option,
        } satisfies RemoteProfileOptionsBuilder

        const result = await update.mutateAsync({
          uid: profile.uid,
          profile: {
            ...profile,
            option,
          },
        })
        updates.push(Promise.resolve(result || undefined))
      }
      try {
        await Promise.all(updates)
      } catch (e) {
        message(`failed to update profiles: \n${formatError(e)}`, {
          kind: 'error',
        })
      }
    })
  })

  return (
    <SidePage
      title={t('Profiles')}
      flexReverse
      header={
        <div className="flex items-center gap-2">
          <RuntimeConfigDiffDialog
            open={runtimeConfigViewerOpen}
            onClose={() => setRuntimeConfigViewerOpen(false)}
          />
          <IconButton
            className="h-10 w-10"
            color="inherit"
            title={t('Runtime Config')}
            onClick={() => {
              setRuntimeConfigViewerOpen(true)
            }}
          >
            <MdiTextBoxCheckOutline
            // style={{
            //   color: theme.palette.text.primary,
            // }}
            />
          </IconButton>
          <Badge
            variant="dot"
            color={
              maxLogLevelTriggered.global === 'error'
                ? 'error'
                : maxLogLevelTriggered.global === 'warn'
                  ? 'warning'
                  : 'primary'
            }
            invisible={!maxLogLevelTriggered.global}
          >
            <Button
              size="small"
              variant={globalChain ? 'contained' : 'outlined'}
              onClick={handleGlobalChainClick}
              startIcon={<Public />}
            >
              {t('Global Proxy Chains')}
            </Button>
          </Badge>
        </div>
      }
      side={hasSide && <ProfileSide onClose={handleSideClose} />}
    >
      <AnimatePresence initial={false} mode="sync">
        <GlobalUpdatePendingContext.Provider value={globalUpdatePending}>
          <div className="flex flex-col gap-4 p-6">
            <QuickImport />

            {profiles && (
              <Grid container spacing={2}>
                {profiles.clash?.map((item) => (
                  <Grid
                    key={item.uid}
                    size={{
                      xs: 12,
                      sm: 12,
                      md: hasSide && width <= 1000 ? 12 : 6,
                      lg: 4,
                      xl: 3,
                    }}
                  >
                    <motion.div
                      key={item.uid}
                      layoutId={`profile-${item.uid}`}
                      layout="position"
                      initial={false}
                    >
                      <ProfileItem
                        item={item}
                        onClickChains={onClickChains}
                        selected={query.data?.current?.includes(item.uid)}
                        maxLogLevelTriggered={maxLogLevelTriggered}
                        chainsSelected={chainsSelected === item.uid}
                      />
                    </motion.div>
                  </Grid>
                ))}
              </Grid>
            )}
          </div>
        </GlobalUpdatePendingContext.Provider>
      </AnimatePresence>

      <AddProfileContext.Provider value={addProfileCtxValue}>
        <div className="fixed right-8 bottom-8">
          <FloatingButton
            className="relative -top-3 -right-2.5 flex size-11 min-w-fit"
            sx={[
              (theme) => ({
                backgroundColor: theme.palette.grey[200],
                boxShadow: 4,
                '&:hover': {
                  backgroundColor: theme.palette.grey[300],
                },
                ...theme.applyStyles('dark', {
                  backgroundColor: theme.palette.grey[800],
                  '&:hover': {
                    backgroundColor: theme.palette.grey[700],
                  },
                }),
              }),
            ]}
            onClick={handleGlobalProfileUpdate}
          >
            {globalUpdatePending ? <CircularProgress size={22} /> : <Update />}
          </FloatingButton>
          <NewProfileButton className="static" />
        </div>
      </AddProfileContext.Provider>
    </SidePage>
  )
}
