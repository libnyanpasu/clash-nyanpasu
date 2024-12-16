import { useTheme } from '@mui/material'

export const useColorForDelay = (delay: number): string => {
  const { palette } = useTheme()

  const delayColorMapping: { [key: string]: string } = {
    '-1': palette.text.primary,
    '0': palette.text.secondary,
    '1': palette.text.secondary,
    '500': palette.success.main,
    '2000': palette.warning.main,
    '10000': palette.error.main,
  }

  let color: string = palette.text.secondary

  for (const key in delayColorMapping) {
    if (delay <= parseInt(key)) {
      color = delayColorMapping[key]
      break
    }
  }

  return color
}
