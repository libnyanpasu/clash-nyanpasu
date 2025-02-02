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
import LoadingButton from '@mui/lab/LoadingButton'
import ListItem from '@mui/material/ListItem'
import ListItemButton from '@mui/material/ListItemButton'
import { alpha, useTheme } from '@mui/material/styles'
import Tooltip from '@mui/material/Tooltip'
import {
  ClashCore,
  Core,
  InspectUpdater,
  inspectUpdater,
  useNyanpasu,
} from '@nyanpasu/interface'
import { cleanDeepClickEvent, cn } from '@nyanpasu/ui'

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
  const { palette } = useTheme()

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
    <motion.div
      className={cn(
        'absolute top-0 left-0 z-10 h-full w-full rounded-2xl backdrop-blur',
        'flex flex-col items-center justify-center gap-2',
      )}
      style={{
        backgroundColor: alpha(palette.primary.main, 0.3),
      }}
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
      <div
        className="absolute left-0 h-full rounded-2xl transition-all"
        style={{
          backgroundColor: alpha(palette.primary.main, 0.3),
          width: `${calcProgress(data) < 10 ? 10 : calcProgress(data)}%`,
        }}
      />

      <div className="truncate capitalize">{parsedState()}</div>

      <div className="truncate">
        {calcProgress(data).toFixed(0)}%{''}
        <span>({parseTraffic(data?.downloader.speed || 0)}/s)</span>
      </div>
    </motion.div>
  )
}

export interface ClashCoreItemProps {
  selected: boolean
  data: Core
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
  onClick,
}: ClashCoreItemProps) => {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const { updateCore, getClashCore } = useNyanpasu()

  const haveNewVersion = data.latest ? data.latest !== data.version : false

  const [downloadState, setDownloadState] = useState(false)

  const [updater, setUpdater] = useState<InspectUpdater>()

  const handleUpdateCore = async () => {
    try {
      setDownloadState(true)

      const updaterId = await updateCore(data.core)

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

      getClashCore.mutate()

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
        sx={{
          borderRadius: '16px',
          backgroundColor: alpha(palette.background.paper, 0.3),

          '&.Mui-selected': {
            backgroundColor: alpha(palette.primary.main, 0.3),
          },
        }}
        selected={selected}
        onClick={() => {
          if (!downloadState) {
            onClick(data.core)
          }
        }}
      >
        <CardProgress data={updater} show={downloadState} />

        <div className="flex w-full items-center gap-2 p-4">
          <img style={{ width: '64px' }} src={getImage(data.core)} />

          <div className="flex-1">
            <div className="truncate font-bold">
              {data.name}

              {haveNewVersion && (
                <FiberManualRecord
                  sx={{ height: 10, fill: palette.success.main }}
                />
              )}
            </div>

            <div className="truncate text-sm">{data.version}</div>

            {haveNewVersion && (
              <div className="truncate text-sm">New: {data.latest}</div>
            )}
          </div>

          {haveNewVersion && (
            <Tooltip title={t('Update Core')}>
              <LoadingButton
                variant="text"
                className="!size-8 !min-w-0"
                loading={downloadState}
                onClick={(e) => {
                  cleanDeepClickEvent(e)
                  handleUpdateCore()
                }}
              >
                <Update />
              </LoadingButton>
            </Tooltip>
          )}
        </div>
      </ListItemButton>
    </ListItem>
  )
}
