import dayjs from 'dayjs'
import { useAtomValue } from 'jotai'
import { isObject } from 'lodash-es'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'
import { atomIsDrawer } from '@/store'
import {
  alpha,
  CircularProgress,
  Paper,
  Tooltip,
  useTheme,
} from '@mui/material'
import Grid from '@mui/material/Grid2'
import { getCoreStatus, useNyanpasu } from '@nyanpasu/interface'

export const ServiceShortcuts = () => {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const isDrawer = useAtomValue(atomIsDrawer)

  const {
    getServiceStatus: { data: serviceStatus },
  } = useNyanpasu()

  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  })

  const status = useMemo(() => {
    switch (serviceStatus) {
      case 'running': {
        return {
          label: t('running'),
          color: alpha(palette.success[palette.mode], 0.3),
        }
      }

      case 'stopped': {
        return {
          label: t('stopped'),
          color: alpha(palette.error[palette.mode], 0.3),
        }
      }

      case 'not_installed':
      default: {
        return {
          label: t('not_installed'),
          color:
            palette.mode === 'light'
              ? palette.grey[100]
              : palette.background.paper,
        }
      }
    }
  }, [
    serviceStatus,
    t,
    palette.success,
    palette.mode,
    palette.error,
    palette.grey,
    palette.background.paper,
  ])

  const coreStatus = useMemo(() => {
    const status = coreStatusSWR.data || [{ Stopped: null }, 0, 'normal']
    if (
      isObject(status[0]) &&
      Object.prototype.hasOwnProperty.call(status[0], 'Stopped')
    ) {
      const { Stopped } = status[0]
      return {
        label:
          !!Stopped && Stopped.trim()
            ? t('stopped_reason', { reason: Stopped })
            : t('stopped'),
        color: alpha(palette.success[palette.mode], 0.3),
      }
    }
    return {
      label: t('service_shortcuts.core_started_by', {
        by: t(status[2] === 'normal' ? 'UI' : 'service'),
      }),
      color: alpha(palette.success[palette.mode], 0.3),
    }
  }, [coreStatusSWR.data, palette.mode, palette.success, t])

  return (
    <Grid
      size={{
        sm: isDrawer ? 6 : 12,
        md: 6,
        lg: 4,
        xl: 3,
      }}
    >
      <Paper className="flex !h-full flex-col justify-between gap-2 !rounded-3xl p-3">
        {serviceStatus ? (
          <>
            <div className="text-center font-bold">
              {t('service_shortcuts.title')}
            </div>

            <div className="flex w-full flex-col gap-2">
              <div
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                style={{ backgroundColor: status.color }}
              >
                <div>{t('service_shortcuts.service_status')}</div>
                <div>{t(status.label)}</div>
              </div>

              <div
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                style={{ backgroundColor: coreStatus.color }}
              >
                <div>{t('service_shortcuts.core_status')}</div>
                <Tooltip
                  title={
                    !!coreStatusSWR.data?.[1] &&
                    t('service_shortcuts.last_status_changed_since', {
                      time: dayjs(coreStatusSWR.data[1]).fromNow(),
                    })
                  }
                >
                  <div>{coreStatus.label}</div>
                </Tooltip>
              </div>
            </div>
          </>
        ) : (
          <div className="flex w-full flex-col items-center justify-center gap-2">
            <CircularProgress />

            <div>Loading...</div>
          </div>
        )}
      </Paper>
    </Grid>
  )
}

export default ServiceShortcuts
