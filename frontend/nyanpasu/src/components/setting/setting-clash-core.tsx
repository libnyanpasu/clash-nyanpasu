import { useLockFn, useReactive } from 'ahooks'
import { motion } from 'framer-motion'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { OS } from '@/consts'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Box, List, ListItem } from '@mui/material'
import {
  ClashCore,
  useClash,
  useClashConnections,
  useClashCores,
  useClashVersion,
  useSetting,
} from '@nyanpasu/interface'
import { BaseCard, ExpandMore, LoadingButton } from '@nyanpasu/ui'
import { ClashCoreItem } from './modules/clash-core'

export const SettingClashCore = () => {
  const { t } = useTranslation()

  const loading = useReactive({
    mask: false,
  })

  const [expand, setExpand] = useState(false)

  const { value: currentCore } = useSetting('clash_core')

  const {
    query: clashCores,
    upsert: switchCore,
    restartSidecar,
    fetchRemote,
  } = useClashCores()

  const { data: clashVersion } = useClashVersion()

  const { deleteConnections } = useClashConnections()

  const version = useMemo(() => {
    return clashVersion?.premium
      ? `${clashVersion.version} Premium`
      : clashVersion?.meta
        ? `${clashVersion.version} Meta`
        : clashVersion?.version || '-'
  }, [clashVersion])

  const changeClashCore = useLockFn(async (core: ClashCore) => {
    try {
      loading.mask = true
      try {
        await deleteConnections.mutateAsync(undefined)
      } catch (e) {
        console.error(e)
      }

      await switchCore.mutateAsync(core)

      message(
        t('Successfully switched to the clash core', { core: `${core}` }),
        {
          kind: 'info',
          title: t('Successful'),
        },
      )
    } catch (e) {
      message(
        t('Failed to switch. You could see the details in the log', {
          error: `${e instanceof Error ? e.message : String(e)}`,
        }),
        {
          kind: 'error',
          title: t('Error'),
        },
      )
    } finally {
      loading.mask = false
    }
  })

  const handleRestart = async () => {
    try {
      await restartSidecar()

      message(t('Successfully restarted the core'), {
        kind: 'info',
        title: t('Successful'),
      })
    } catch (e) {
      message(
        t('Failed to restart. You could see the details in the log') +
          formatError(e),
        {
          kind: 'error',
          title: t('Error'),
        },
      )
    }
  }

  const handleCheckUpdates = async () => {
    try {
      await fetchRemote.mutateAsync()
    } catch (e) {
      message(
        t('Failed to fetch. Please check your network connection') +
          '\n' +
          formatError(e),
        {
          kind: 'error',
          title: t('Error'),
        },
      )
    }
  }

  return (
    <BaseCard
      label={t('Clash Core')}
      loading={loading.mask}
      labelChildren={<span>{version}</span>}
    >
      <List disablePadding>
        {clashCores.data &&
          Object.entries(clashCores.data).map(([core, item]) => {
            const show = expand || core === currentCore

            return (
              <motion.div
                key={item.name}
                animate={show ? 'open' : 'closed'}
                variants={{
                  open: {
                    height: 'auto',
                    opacity: 1,
                    scale: 1,
                  },
                  closed: {
                    height: 0,
                    opacity: 0,
                    scale: 0.7,
                  },
                }}
                transition={{
                  type: 'spring',
                  bounce: 0,
                  duration: 0.35,
                }}
              >
                <ClashCoreItem
                  data={item}
                  core={core as ClashCore}
                  selected={core === currentCore}
                  onClick={() => changeClashCore(core as ClashCore)}
                />
              </motion.div>
            )
          })}

        <ListItem
          sx={{
            pl: 0,
            pr: 0,
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <Box display="flex" gap={1}>
            <LoadingButton variant="outlined" onClick={handleRestart}>
              {t('Restart')}
            </LoadingButton>

            {/** TODO: Support Linux when Manifest v2 released */}
            {OS !== 'linux' && (
              <LoadingButton
                variant="contained"
                loading={fetchRemote.isPending}
                onClick={handleCheckUpdates}
              >
                {t('Check Updates')}
              </LoadingButton>
            )}
          </Box>

          <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
        </ListItem>
      </List>
    </BaseCard>
  )
}

export default SettingClashCore
