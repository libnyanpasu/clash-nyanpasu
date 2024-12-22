import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiButton: Components<Theme>['MuiButton'] = {
  styleOverrides: {
    root: {
      borderRadius: '48px',
    },
  },
}
