import { motion } from 'framer-motion'
import { isObject } from 'lodash-es'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import ClashRs from '@/assets/image/core/clash-rs.png'
import ClashMeta from '@/assets/image/core/clash.meta.png'
import Clash from '@/assets/image/core/clash.png'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import parseTraffic from '@/utils/parse-traffic'
import FiberManualRecord from '@mui/icons-material/FiberManualRecord'
import Update from '@mui/icons-material/Update'
import { Box } from '@mui/material'
import ListItem from '@mui/material/ListItem'
import ListItemButton from '@mui/material/ListItemButton'
import Tooltip from '@mui/material/Tooltip'
import {
  ClashCore,
  ClashCoresDetail,
  InspectUpdater,
  inspectUpdater,
  useClashCores,
} from '@nyanpasu/interface'
import {
  alpha,
  MUIButton as Button,
  cleanDeepClickEvent,
  cn,
} from '@nyanpasu/ui'

export const getImage = (core: ClashCore) => {
  switch (core) {
    case 'mihomo':
    case 'mihomo-alpha': {
      return ClashMeta
    }

    case 'clash-rs':
    case 'clash-rs-alpha': {
      return ClashRs
    }

    default: {
      return Clash
    }
  }
}

const calcProgress = (data?: InspectUpdater) => {
  return (
    (Number(data?.downloader?.downloaded) / Number(data?.downloader?.total)) *
    100
  )
}

const CardProgress = ({
  data,
  show,
}: {
  data?: InspectUpdater
  show?: boolean
}) => {
  const parsedState = () => {
    if (data?.downloader?.state) {
      return 'waiting'
    } else if (isObject(data?.downloader.state)) {
      return data?.downloader.state.failed
    } else {
      return data?.downloader.state
    }
  }

  return (
    <Box
      component={motion.div}
      className={cn(
        'absolute top-0 left-0 z-10 h-full w-full rounded-2xl backdrop-blur',
        'flex flex-col items-center justify-center gap-2',
      )}
      sx={(theme) => ({
        backgroundColor: alpha(theme.vars.palette.primary.main, 0.3),
      })}
      animate={show ? 'open' : 'closed'}
      initial={{ opacity: 0 }}
      variants={{
        open: {
          opacity: 1,
          display: 'flex',
        },
        closed: {
          opacity: 0,
          transitionEnd: {
            display: 'none',
          },
        },
      }}
    >
      <Box
        className="absolute left-0 h-full rounded-2xl transition-all"
        sx={(theme) => ({
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.3),
          width: `${calcProgress(data) < 10 ? 10 : calcProgress(data)}%`,
        })}
      />

      <div className="truncate capitalize">{parsedState()}</div>

      <div className="truncate">
        {calcProgress(data).toFixed(0)}%{''}
        <span>({parseTraffic(data?.downloader.speed || 0)}/s)</span>
      </div>
    </Box>
  )
}

export interface ClashCoreItemProps {
  selected: boolean
  data: ClashCoresDetail
  core: ClashCore
  onClick: (core: ClashCore) => void
}

/**
 * @example
 * <ClashCoreItem
    data={core}
    selected={selected}
    onClick={() => changeClashCore(item.core)}
  />
 *
 * `Design for Clash Core used.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const ClashCoreItem = ({
  selected,
  data,
  core,
  onClick,
}: ClashCoreItemProps) => {
  const { t } = useTranslation()

  const { query, updateCore } = useClashCores()

  const haveNewVersion = data.latestVersion
    ? data.latestVersion !== data.currentVersion
    : false

  const [downloadState, setDownloadState] = useState(false)

  const [updater, setUpdater] = useState<InspectUpdater>()

  const handleUpdateCore = async () => {
    try {
      setDownloadState(true)

      const updaterId = await updateCore.mutateAsync(core)

      if (!updaterId) {
        throw new Error('Failed to update')
      }

      await new Promise<void>((resolve, reject) => {
        const interval = setInterval(async () => {
          const result = await inspectUpdater(updaterId)

          setUpdater(result)

          if (
            isObject(result.downloader.state) &&
            Object.prototype.hasOwnProperty.call(
              result.downloader.state,
              'failed',
            )
          ) {
            reject(result.downloader.state.failed)
            clearInterval(interval)
          }

          if (result.state === 'done') {
            resolve()
            clearInterval(interval)
          }
        }, 100)
      })

      await query.refetch()

      message(t('Successfully updated the core', { core: `${data.name}` }), {
        kind: 'info',
        title: t('Successful'),
      })
    } catch (e) {
      message(t('Failed to update', { error: `${formatError(e)}` }), {
        kind: 'error',
        title: t('Error'),
      })
    } finally {
      setDownloadState(false)
    }
  }

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemButton
        className="!relative !p-0"
        sx={(theme) => ({
          borderRadius: '16px',
          backgroundColor: alpha(theme.vars.palette.background.paper, 0.3),

          '&.Mui-selected': {
            backgroundColor: alpha(theme.vars.palette.primary.main, 0.3),
          },
        })}
        selected={selected}
        onClick={() => {
          if (!downloadState) {
            onClick(core)
          }
        }}
      >
        <CardProgress data={updater} show={downloadState} />

        <div className="flex w-full items-center gap-2 p-4">
          <img style={{ width: '64px' }} src={getImage(core)} />

          <div className="flex-1">
            <div className="truncate font-bold">
              {data.name}

              {haveNewVersion && (
                <FiberManualRecord
                  sx={(theme) => ({
                    height: 10,
                    fill: theme.vars.palette.success.main,
                  })}
                />
              )}
            </div>

            <div className="truncate text-sm">{data.currentVersion}</div>

            {haveNewVersion && (
              <div className="truncate text-sm">New: {data.latestVersion}</div>
            )}
          </div>

          {haveNewVersion && (
            <Tooltip title={t('Update Core')}>
              <Button
                variant="text"
                className="!size-8 !min-w-0"
                loading={downloadState}
                onClick={(e) => {
                  cleanDeepClickEvent(e)
                  handleUpdateCore()
                }}
              >
                <Update />
              </Button>
            </Tooltip>
          )}
        </div>
      </ListItemButton>
    </ListItem>
  )
}
