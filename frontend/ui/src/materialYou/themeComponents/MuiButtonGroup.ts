import { alpha, darken, Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiButtonGroup: Components<Theme>['MuiButtonGroup'] = {
  styleOverrides: {
    grouped: ({ theme }) => ({
      fontWeight: 700,
      height: '2.5em',
      padding: '0 1.25em',
      border: `1px solid ${darken(theme.palette.primary.main, 0.09)}`,
      color: darken(theme.palette.primary.main, 0.2),

      '&.MuiButton-contained.MuiButton-colorPrimary': {
        boxShadow: 'none',
        border: `1px solid ${theme.palette.primary.mainChannel}`,
        backgroundColor: alpha(theme.palette.primary.main, 0.2),
        color: theme.palette.primary.main,
        '&::before': {
          content: 'none',
        },
        '&:hover': {
          backgroundColor: alpha(theme.palette.primary.main, 0.3),
        },
      },
    }),
    firstButton: {
      borderTopLeftRadius: 48,
      borderBottomLeftRadius: 48,

      '&.MuiButton-sizeSmall': {
        paddingLeft: '1.5em',
      },

      '&.MuiButton-sizeMedium': {
        paddingLeft: '20px',
      },

      '&.MuiButton-sizeLarge': {
        paddingLeft: '26px',
      },
    },
    lastButton: {
      borderTopRightRadius: 48,
      borderBottomRightRadius: 48,

      '&.MuiButton-sizeSmall': {
        paddingRight: '1.5em',
      },

      '&.MuiButton-sizeMedium': {
        paddingRight: '20px',
      },

      '&.MuiButton-sizeLarge': {
        paddingRight: '26px',
      },
    },
  },
} satisfies Components<Theme>['MuiButtonGroup']
