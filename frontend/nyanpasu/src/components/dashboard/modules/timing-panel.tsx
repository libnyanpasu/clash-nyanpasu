import { useAtomValue } from 'jotai'
import { useTranslation } from 'react-i18next'
import { useColorSxForDelay } from '@/hooks/theme'
import { atomIsDrawer } from '@/store'
import { Box, Paper } from '@mui/material'
import Grid from '@mui/material/Grid'
import { useSetting } from '@nyanpasu/interface'

function LatencyTag({ name, value }: { name: string; value: number }) {
  const { t } = useTranslation()

  const sx = useColorSxForDelay(value)

  return (
    <div className="flex justify-between gap-1">
      <div className="font-bold">{name}:</div>

      <Box className="truncate" sx={sx}>
        {value ? `${value.toFixed(0)} ms` : t('Timeout')}
      </Box>
    </div>
  )
}

export const TimingPanel = ({ data }: { data: { [key: string]: number } }) => {
  const isDrawer = useAtomValue(atomIsDrawer)

  const { value } = useSetting('clash_core')

  const supportMemory = value && ['mihomo', 'mihomo-alpha'].includes(value)

  return (
    <Grid
      size={{
        sm: isDrawer ? 6 : 12,
        md: supportMemory ? 4 : 6,
        lg: supportMemory ? 3 : 4,
        xl: 3,
      }}
    >
      <Paper className="!h-full !rounded-3xl p-4">
        <div className="flex h-full flex-col justify-between">
          {Object.entries(data).map(([name, value]) => (
            <LatencyTag key={name} name={name} value={value} />
          ))}
        </div>
      </Paper>
    </Grid>
  )
}

export default TimingPanel
