import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'
import type {} from '@mui/material/themeCssVarsAugmentation'

export const MuiSwitch: Components<Theme>['MuiSwitch'] = {
  styleOverrides: {
    root: ({ theme }) => ({
      padding: 0,
      margin: 0,

      '& .Mui-checked': {
        '& .MuiSwitch-thumb': {
          color: theme.palette.grey.A100,
        },
      },

      '&:has(.Mui-checked) .MuiSwitch-track::before': {
        opacity: 0,
      },

      '&:has(.Mui-disabled) .MuiSwitch-track': {
        opacity: '0.5 !important',
        cursor: 'not-allowed',
      },

      variants: [
        {
          props: {
            size: 'medium',
          },
          style: {
            height: 32,

            '& .MuiSwitch-switchBase': {
              padding: '6px',
            },

            '& .MuiSwitch-thumb': {
              width: 14,
              height: 14,
              margin: 3,
            },

            '& .Mui-checked': {
              '&.MuiSwitch-switchBase': {
                marginLeft: '6px',
              },

              '& .MuiSwitch-thumb': {
                width: 24,
                height: 24,
                margin: -2,
              },
            },
          },
        },
        {
          props: {
            size: 'small',
          },
          style: {
            height: 24,

            '& .MuiSwitch-switchBase': {
              padding: '3px',
            },

            '& .MuiSwitch-thumb': {
              width: 12,
              height: 12,
              margin: 3,
            },

            '& .Mui-checked': {
              '&.MuiSwitch-switchBase': {
                marginLeft: '1px',
              },

              '& .MuiSwitch-thumb': {
                width: 17,
                height: 17,
                margin: 0,
              },
            },
          },
        },
      ],
    }),

    track: ({ theme }) => ({
      borderRadius: '48px',
      backgroundColor: theme.palette.grey.A200,
      opacity: `1 !important`,

      ...theme.applyStyles('dark', {
        backgroundColor: theme.palette.grey.A700,
        opacity: `0.7 !important`,
      }),

      '&::before': {
        content: '""',
        border: `solid 2px ${theme.palette.grey.A700}`,
        width: '100%',
        height: '100%',
        opacity: 1,
        position: 'absolute',
        borderRadius: 'inherit',
        boxSizing: 'border-box',
        transitionProperty: 'opacity, background-color',
        transitionTimingFunction: 'linear',
        transitionDuration: '100ms',
      },
    }),

    thumb: ({ theme }) => ({
      boxShadow: 'none',
      color: theme.palette.grey.A700,

      ...theme.applyStyles('dark', {
        backgroundColor: theme.palette.grey.A200,
      }),
    }),
  },
}
