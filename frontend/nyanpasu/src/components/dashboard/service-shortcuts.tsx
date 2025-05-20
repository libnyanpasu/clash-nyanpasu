import dayjs from 'dayjs'
import { useAtomValue } from 'jotai'
import { isObject } from 'lodash-es'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'
import { atomIsDrawer } from '@/store'
import {
  Box,
  CircularProgress,
  Paper,
  SxProps,
  Theme,
  Tooltip,
} from '@mui/material'
import Grid from '@mui/material/Grid'
import { getCoreStatus, useSystemService } from '@nyanpasu/interface'
import { alpha } from '@nyanpasu/ui'

type Status = {
  label: string
  sx: SxProps<Theme>
}

export const ServiceShortcuts = () => {
  const { t } = useTranslation()

  const isDrawer = useAtomValue(atomIsDrawer)

  const {
    query: { data: serviceStatus },
  } = useSystemService()

  // TODO: refactor to use tanstack query
  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  })

  const status: Status = useMemo(() => {
    switch (serviceStatus?.status) {
      case 'running': {
        return {
          label: t('running'),
          sx: (theme) => ({
            backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
            ...theme.applyStyles('dark', {
              backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
            }),
          }),
        }
      }

      case 'stopped': {
        return {
          label: t('stopped'),
          sx: (theme) => ({
            backgroundColor: alpha(theme.vars.palette.error.light, 0.3),
            ...theme.applyStyles('dark', {
              backgroundColor: alpha(theme.vars.palette.error.dark, 0.3),
            }),
          }),
        }
      }

      case 'not_installed':
      default: {
        return {
          label: t('not_installed'),
          sx: (theme) => ({
            backgroundColor: theme.vars.palette.grey[100],
            ...theme.applyStyles('dark', {
              backgroundColor: theme.vars.palette.background.paper,
            }),
          }),
        }
      }
    }
  }, [serviceStatus, t])

  const coreStatus: Status = useMemo(() => {
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
        sx: (theme) => ({
          backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
          ...theme.applyStyles('dark', {
            backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
          }),
        }),
      }
    }
    return {
      label: t('service_shortcuts.core_started_by', {
        by: t(status[2] === 'normal' ? 'UI' : 'service'),
      }),
      sx: (theme) => ({
        backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
        ...theme.applyStyles('dark', {
          backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
        }),
      }),
    }
  }, [coreStatusSWR.data, t])

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
              <Box
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                sx={status.sx}
              >
                <div>{t('service_shortcuts.service_status')}</div>
                <div>{t(status.label)}</div>
              </Box>

              <Box
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                sx={coreStatus.sx}
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
              </Box>
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
