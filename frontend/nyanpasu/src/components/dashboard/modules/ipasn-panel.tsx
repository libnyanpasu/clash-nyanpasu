import countryCodeEmoji from 'country-code-emoji'
import { useAtomValue } from 'jotai'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'
import { atomIsDrawer } from '@/store'
import { Visibility, VisibilityOff } from '@mui/icons-material'
import { LoadingButton } from '@mui/lab'
import { CircularProgress, IconButton, Paper, Tooltip } from '@mui/material'
import Grid from '@mui/material/Grid2'
import { useIPSB } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

const IP_REFRESH_SECONDS = 180

const EmojiCounty = ({ countryCode }: { countryCode: string }) => {
  const emoji = countryCodeEmoji(countryCode)

  return (
    <div className="relative text-5xl">
      <span className="opacity-50 blur">{emoji}</span>

      <span className="absolute left-0">{emoji}</span>
    </div>
  )
}

const MAX_WIDTH = 'calc(100% - 48px - 16px)'

export const IPASNPanel = ({ refreshCount }: { refreshCount: number }) => {
  const { t } = useTranslation()

  const { data, mutate, isValidating, isLoading } = useIPSB()

  const handleRefreshIP = () => {
    mutate()
  }

  const [showIPAddress, setShowIPAddress] = useState(false)

  const isDrawer = useAtomValue(atomIsDrawer)

  return (
    <Grid
      size={{
        sm: isDrawer ? 7 : 12,
        md: 8,
        lg: 5,
        xl: 3,
      }}
    >
      <Paper className="relative flex !h-full gap-4 !rounded-3xl px-4 py-3 select-text">
        {data ? (
          <>
            {data.country_code && (
              <EmojiCounty countryCode={data.country_code} />
            )}

            <div className="flex flex-col gap-1" style={{ width: MAX_WIDTH }}>
              <div className="text-shadow-md flex items-end justify-between text-xl font-bold">
                <div className="truncate">{data.country}</div>

                <Tooltip title={t('Click to Refresh Now')}>
                  <LoadingButton
                    className="!size-8 !min-w-0"
                    onClick={handleRefreshIP}
                    loading={isValidating}
                  >
                    {!isValidating && (
                      <CircularProgress
                        size={16}
                        variant="determinate"
                        value={
                          100 -
                          ((refreshCount % IP_REFRESH_SECONDS) /
                            IP_REFRESH_SECONDS) *
                            100
                        }
                      />
                    )}
                  </LoadingButton>
                </Tooltip>
              </div>

              <div className="truncate">{data.organization}</div>

              <div className="text-sm">AS{data.asn}</div>

              <div className="flex items-center justify-between gap-4">
                <div
                  className="relative font-mono"
                  style={{ width: MAX_WIDTH }}
                >
                  <span
                    className={cn(
                      'block truncate transition-opacity',
                      showIPAddress ? 'opacity-100' : 'opacity-0',
                    )}
                  >
                    {data.ip}
                  </span>

                  <span
                    className={cn(
                      'absolute top-0 left-0 block h-full w-full rounded-lg bg-slate-300 transition-opacity',
                      showIPAddress ? 'opacity-0' : 'animate-pulse opacity-100',
                    )}
                  />
                </div>

                <IconButton
                  className="!size-8"
                  color="primary"
                  onClick={() => setShowIPAddress(!showIPAddress)}
                >
                  {showIPAddress ? <Visibility /> : <VisibilityOff />}
                </IconButton>
              </div>
            </div>
          </>
        ) : (
          <>
            <div className="mt-1.5 mb-2 h-9 w-12 animate-pulse rounded-lg bg-slate-700" />

            <div className="flex flex-1 animate-pulse flex-col gap-1">
              <div className="mt-1.5 h-6 w-20 rounded-full bg-slate-700" />

              <div className="mt-1 h-5 w-44 rounded-full bg-slate-700" />

              <div className="mt-1 h-5 w-16 rounded-full bg-slate-700" />

              <div className="mt-1 h-6 w-32 rounded-lg bg-slate-700" />
            </div>
          </>
        )}
      </Paper>
    </Grid>
  )
}

export default IPASNPanel
