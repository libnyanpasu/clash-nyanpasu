import { useLockFn, useMemoizedFn, useSetState } from 'ahooks'
import dayjs from 'dayjs'
import { AnimatePresence, motion } from 'framer-motion'
import { memo, use, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import parseTraffic from '@/utils/parse-traffic'
import {
  FiberManualRecord,
  FilterDrama,
  InsertDriveFile,
  Menu as MenuIcon,
  Terminal,
  Update,
} from '@mui/icons-material'
import LoadingButton from '@mui/lab/LoadingButton'
import {
  alpha,
  Badge,
  Button,
  Chip,
  LinearProgress,
  Menu,
  MenuItem,
  Paper,
  Tooltip,
  useTheme,
} from '@mui/material'
import {
  Profile,
  ProfileQueryResultItem,
  RemoteProfile,
  RemoteProfileOptions,
  useClash,
  useProfile,
} from '@nyanpasu/interface'
import { cleanDeepClickEvent, cn } from '@nyanpasu/ui'
import { ProfileDialog } from './profile-dialog'
import { GlobalUpdatePendingContext } from './provider'

export interface ProfileItemProps {
  item: ProfileQueryResultItem
  selected?: boolean
  maxLogLevelTriggered?: {
    global: undefined | 'info' | 'error' | 'warn'
    current: undefined | 'info' | 'error' | 'warn'
  }
  onClickChains: (item: Profile) => void
  chainsSelected?: boolean
}

export const ProfileItem = memo(function ProfileItem({
  item,
  selected,
  onClickChains,
  chainsSelected,
  maxLogLevelTriggered,
}: ProfileItemProps) {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const { deleteConnections } = useClash()

  const { upsert } = useProfile()

  const globalUpdatePending = use(GlobalUpdatePendingContext)

  const [loading, setLoading] = useSetState({
    update: false,
    card: false,
  })

  const calc = () => {
    let progress = 0
    let total = 0
    let used = 0

    if ('extra' in item && item.extra) {
      const { download, upload, total: t } = item.extra

      total = t

      used = download + upload

      progress = (used / total) * 100
    }

    return { progress, total, used }
  }

  const { progress, total, used } = calc()

  const isRemote = item.type === 'remote'

  const IconComponent = isRemote ? FilterDrama : InsertDriveFile

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null)

  const handleSelect = useLockFn(async () => {
    if (selected) {
      return
    }

    try {
      setLoading({ card: true })

      await upsert.mutateAsync({ current: [item.uid] })

      await deleteConnections()
    } catch (err) {
      const isFetchError = err instanceof Error && err.name === 'FetchError'
      message(
        isFetchError
          ? t('FetchError', {
              content: t('Subscription'),
            })
          : `Error setting profile: \n ${err instanceof Error ? err.message : String(err)}`,
        {
          title: t('Error'),
          kind: 'error',
        },
      )
    } finally {
      setLoading({ card: false })
    }
  })

  const handleUpdate = useLockFn(async (proxy?: boolean) => {
    // TODO: define backend serde(option) to move null
    const selfOption = 'option' in item ? item.option : undefined

    const options: RemoteProfileOptions = {
      with_proxy: false,
      self_proxy: false,
      update_interval: 0,
      ...selfOption,
    }

    if (proxy) {
      if (selfOption?.self_proxy) {
        options.with_proxy = false
        options.self_proxy = true
      } else {
        options.with_proxy = true
        options.self_proxy = false
      }
    }

    try {
      setLoading({ update: true })

      await item?.update?.(item)
    } finally {
      setLoading({ update: false })
    }
  })

  const handleDelete = useLockFn(async () => {
    try {
      // await deleteProfile(item.uid)
      await item?.drop?.()
    } catch (err) {
      message(`Delete failed: \n ${JSON.stringify(err)}`, {
        title: t('Error'),
        kind: 'error',
      })
    }
  })

  const menuMapping = useMemo(
    () => ({
      Select: () => handleSelect(),
      'Edit Info': () => setOpen(true),
      'Proxy Chains': () => onClickChains(item),
      'Open File': () => item?.view?.(),
      Update: () => handleUpdate(),
      'Update(Proxy)': () => handleUpdate(true),
      Delete: () => handleDelete(),
    }),
    [handleDelete, handleSelect, handleUpdate, item, onClickChains],
  )

  const MenuComp = useMemo(() => {
    const handleClick = (func: () => void) => {
      setAnchorEl(null)
      func()
    }

    return (
      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(menuMapping).map(([key, func], index) => {
          return (
            <MenuItem
              key={index}
              onClick={(e) => {
                cleanDeepClickEvent(e)
                handleClick(func)
              }}
            >
              {t(key)}
            </MenuItem>
          )
        })}
      </Menu>
    )
  }, [anchorEl, menuMapping, t])

  const [open, setOpen] = useState(false)

  return (
    <>
      <Paper
        className="relative transition-all"
        sx={[
          {
            borderRadius: 6,
          },
          selected
            ? {
                backgroundColor: alpha(palette.primary.main, 0.2),
              }
            : {
                backgroundColor: null,
              },
        ]}
      >
        <div
          className="flex cursor-pointer flex-col gap-4 p-5"
          onClick={handleSelect}
        >
          <div className="flex items-center justify-between gap-2">
            <Tooltip title={(item as RemoteProfile).url}>
              <Chip
                className="!pr-2 !pl-2 font-bold"
                avatar={<IconComponent className="!size-5" color="primary" />}
                label={isRemote ? t('Remote') : t('Local')}
              />
            </Tooltip>

            {selected && (
              <FiberManualRecord
                className="top-0 mr-auto !size-3 animate-bounce"
                sx={{
                  fill: palette.success.main,
                }}
              />
            )}

            <TextCarousel
              className="flex h-6 w-30 items-center"
              nodes={[
                !!item.updated && (
                  <TimeSpan ts={item.updated!} k="Subscription Updated At" />
                ),
                !!(item as RemoteProfile).extra?.expire && (
                  <TimeSpan
                    ts={(item as RemoteProfile).extra!.expire!}
                    k="Subscription Expires In"
                  />
                ),
              ]}
            />
          </div>

          <div>
            <p className="truncate text-lg font-bold">{item.name}</p>
            <p className="truncate">{item.desc}</p>
          </div>

          <div
            className={cn(
              'flex items-center justify-between gap-4',
              !isRemote && 'invisible',
            )}
          >
            <div className="w-full">
              <LinearProgress variant="determinate" value={progress} />
            </div>

            <Tooltip title={`${parseTraffic(used)} / ${parseTraffic(total)}`}>
              <div className="text-sm font-bold">
                {((used / total) * 100).toFixed(2)}%
              </div>
            </Tooltip>
          </div>

          <div className="flex justify-end gap-2">
            <Badge
              variant="dot"
              color={
                maxLogLevelTriggered?.current === 'error'
                  ? 'error'
                  : maxLogLevelTriggered?.current === 'warn'
                    ? 'warning'
                    : 'primary'
              }
              invisible={!selected || !maxLogLevelTriggered?.current}
            >
              <Button
                className="!mr-auto"
                size="small"
                variant={chainsSelected ? 'contained' : 'outlined'}
                startIcon={<Terminal />}
                onClick={(e) => {
                  cleanDeepClickEvent(e)
                  onClickChains(item)
                }}
              >
                {t('Proxy Chains')}
              </Button>
            </Badge>

            {isRemote && (
              <Tooltip title={t('Update')}>
                <LoadingButton
                  size="small"
                  variant="outlined"
                  className="!size-8 !min-w-0"
                  onClick={(e) => {
                    cleanDeepClickEvent(e)
                    menuMapping.Update()
                  }}
                  loading={globalUpdatePending || loading.update}
                >
                  <Update />
                </LoadingButton>
              </Tooltip>
            )}

            <Tooltip title={t('Menu')}>
              <Button
                size="small"
                variant="contained"
                className="!size-8 !min-w-0"
                onClick={(e) => {
                  cleanDeepClickEvent(e)
                  setAnchorEl(e.currentTarget)
                }}
              >
                <MenuIcon />
              </Button>
            </Tooltip>
          </div>
        </div>

        <motion.div
          className={cn(
            'absolute top-0 left-0 h-full w-full',
            'flex-col items-center justify-center gap-4',
            'text-shadow-xl rounded-3xl font-bold backdrop-blur',
          )}
          initial={{ opacity: 0, display: 'none' }}
          animate={loading.card ? 'show' : 'hidden'}
          variants={{
            show: { opacity: 1, display: 'flex' },
            hidden: { opacity: 0, transitionEnd: { display: 'none' } },
          }}
        >
          <LinearProgress className="w-40" />

          <div>{t('Applying Profile')}</div>
        </motion.div>
      </Paper>
      {MenuComp}
      <ProfileDialog
        open={open}
        onClose={() => setOpen(false)}
        profile={item}
      />
    </>
  )
})

function TimeSpan({ ts, k }: { ts: number; k: string }) {
  const time = dayjs(ts * 1000)
  const { t } = useTranslation()
  return (
    <Tooltip title={time.format('YYYY/MM/DD HH:mm:ss')}>
      <div className="animate-marquee h-fit text-right text-sm font-medium whitespace-nowrap">
        {t(k, {
          time: time.fromNow(),
        })}
      </div>
    </Tooltip>
  )
}

function TextCarousel(props: { nodes: React.ReactNode[]; className?: string }) {
  const [index, setIndex] = useState(0)
  const nodes = useMemo(
    () => props.nodes.filter((item) => !!item),
    [props.nodes],
  )

  const nextNode = useMemoizedFn(() => {
    setIndex((i) => (i + 1) % nodes.length)
  })

  useEffect(() => {
    if (nodes.length <= 1) {
      return
    }
    const timer = setInterval(() => {
      nextNode()
    }, 8000)
    return () => clearInterval(timer)
  }, [index, nextNode, nodes.length])
  if (nodes.length === 0) {
    return null
  }
  return (
    <div
      className={cn('overflow-hidden', props.className)}
      onClick={() => nextNode()}
    >
      <AnimatePresence mode="wait">
        {nodes.map(
          (node, i) =>
            i === index && (
              <motion.div
                className="h-full w-full"
                key={index}
                initial={{ y: 40, opacity: 0, scale: 0.8 }}
                animate={{ y: 0, opacity: 1, scale: 1 }}
                exit={{ y: -40, opacity: 0, scale: 0.8 }}
              >
                {node}
              </motion.div>
            ),
        )}
      </AnimatePresence>
    </div>
  )
}

export default ProfileItem
