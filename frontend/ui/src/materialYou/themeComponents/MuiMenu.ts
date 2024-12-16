import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiMenu: Components<Theme>['MuiMenu'] = {
  styleOverrides: {
    paper: ({ theme }) => ({
      boxShadow: `${theme.shadows[8]} !important`,
    }),
  },
}
