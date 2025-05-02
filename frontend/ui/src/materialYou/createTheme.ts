import createPalette from 'node_modules/@mui/material/esm/styles/createPalette'
import {
  argbFromHex,
  hexFromArgb,
  themeFromSourceColor,
} from '@material/material-color-utilities'
import { createTheme } from '@mui/material/styles'
import {
  MuiButton,
  MuiButtonGroup,
  MuiCard,
  MuiCardContent,
  MuiDialog,
  MuiDialogActions,
  MuiDialogContent,
  MuiDialogTitle,
  MuiLinearProgress,
  MuiMenu,
  MuiPaper,
  MuiSwitch,
} from './themeComponents'
import { MUI_BREAKPOINTS } from './themeConsts.mjs'

export const createMDYTheme = (color: string, fontFamily?: string) => {
  const materialColor = themeFromSourceColor(argbFromHex(color))

  const generatePalette = (mode: 'light' | 'dark') => {
    return createPalette({
      mode,
      primary: {
        main: hexFromArgb(materialColor.schemes[mode].primary),
      },
      secondary: {
        main: hexFromArgb(materialColor.schemes[mode].secondary),
      },
      error: {
        main: hexFromArgb(materialColor.schemes[mode].error),
      },
      text: {
        primary: hexFromArgb(materialColor.schemes[mode].onPrimaryContainer),
        secondary: hexFromArgb(
          materialColor.schemes[mode].onSecondaryContainer,
        ),
      },
    })
  }
  const colorSchemes = {
    light: {
      palette: generatePalette('light'),
    },
    dark: {
      palette: generatePalette('dark'),
    },
  }
  console.log(colorSchemes)
  const theme = createTheme(
    {
      cssVariables: {
        colorSchemeSelector: 'class',
      },
      colorSchemes: {
        light: true,
        dark: true,
      },
      typography: {
        fontFamily,
      },
      components: {
        MuiButton,
        MuiButtonGroup,
        MuiCard,
        MuiCardContent,
        MuiDialog,
        MuiDialogActions,
        MuiDialogContent,
        MuiDialogTitle,
        MuiLinearProgress,
        MuiMenu,
        MuiPaper,
        MuiSwitch,
      },
      breakpoints: MUI_BREAKPOINTS,
    },
    {
      colorSchemes,
    },
  )

  return theme
}
