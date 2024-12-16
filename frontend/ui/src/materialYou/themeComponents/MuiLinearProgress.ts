import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiLinearProgress: Components<Theme>['MuiLinearProgress'] = {
  styleOverrides: {
    root: {
      height: '8px',
      borderRadius: '8px',
    },
    bar: {
      borderRadius: '8px',
    },
  },
}
