import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiPaper: Components<Theme>['MuiPaper'] = {
  styleOverrides: {
    root: () => ({
      boxShadow: 'none',
    }),
  },
}
