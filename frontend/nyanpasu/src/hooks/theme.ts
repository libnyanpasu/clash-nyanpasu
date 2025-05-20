import { SxProps, Theme } from '@mui/material'

const delayColorMapping: { [key: string]: SxProps<Theme> } = {
  '-1': (theme) => ({ color: theme.vars.palette.text.primary }),
  '0': (theme) => ({ color: theme.vars.palette.text.secondary }),
  '1': (theme) => ({ color: theme.vars.palette.text.secondary }),
  '500': (theme) => ({ color: theme.vars.palette.success.main }),
  '2000': (theme) => ({ color: theme.vars.palette.warning.main }),
  '10000': (theme) => ({ color: theme.vars.palette.error.main }),
}

export const useColorSxForDelay = (delay: number): SxProps<Theme> => {
  let sx: SxProps<Theme> = (theme: Theme) => ({
    color: theme.vars.palette.text.secondary,
  })

  for (const key in delayColorMapping) {
    if (delay <= parseInt(key)) {
      sx = delayColorMapping[key]
      break
    }
  }

  return sx
}
