import { useLockFn, useReactive } from 'ahooks'
import { motion } from 'framer-motion'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { OS } from '@/consts'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Box, List, ListItem } from '@mui/material'
import { ClashCore, useClash, useNyanpasu } from '@nyanpasu/interface'
import { BaseCard, ExpandMore, LoadingButton } from '@nyanpasu/ui'
import { ClashCoreItem } from './modules/clash-core'

export const SettingClashCore = () => {
  const { t } = useTranslation()

  const loading = useReactive({
    mask: false,
  })

  const [expand, setExpand] = useState(false)
  const {
    nyanpasuConfig,
    setClashCore,
    getClashCore,
    restartSidecar,
    getLatestCore,
  } = useNyanpasu({
    onLatestCoreError: (error) => {
      message(`Fetch latest core failed: ${formatError(error)}`, {
        kind: 'error',
        title: t('Error'),
      })
    },
  })

  const { getVersion, deleteConnections } = useClash()

  const version = useMemo(() => {
    const data = getVersion.data

    return data?.premium
      ? `${data.version} Premium`
      : data?.meta
        ? `${data.version} Meta`
        : data?.version || '-'
  }, [getVersion.data])

  const changeClashCore = useLockFn(async (core: ClashCore) => {
    try {
      loading.mask = true
      try {
        await deleteConnections()
      } catch (e) {
        console.error(e)
      }

      await setClashCore(core)

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
      await getLatestCore.mutate()
    } catch (e) {
      message(t('Failed to fetch. Please check your network connection'), {
        kind: 'error',
        title: t('Error'),
      })
    }
  }

  const mergeCores = useMemo(() => {
    return getClashCore.data?.map((item) => {
      const latest = getLatestCore.data?.find(
        (i) => i.core === item.core,
      )?.latest

      return {
        ...item,
        latest,
      }
    })
  }, [getClashCore.data, getLatestCore.data])

  return (
    <BaseCard
      label={t('Clash Core')}
      loading={loading.mask}
      labelChildren={<span>{version}</span>}
    >
      <List disablePadding>
        {mergeCores?.map((item) => {
          const show = expand || item.core === nyanpasuConfig?.clash_core

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
                selected={item.core === nyanpasuConfig?.clash_core}
                onClick={() => changeClashCore(item.core)}
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
                loading={getLatestCore.isLoading}
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
