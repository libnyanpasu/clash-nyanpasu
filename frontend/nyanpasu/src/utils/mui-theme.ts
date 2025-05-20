import { SxProps, Theme } from '@mui/material'

export const mergeSxProps = (...props: Array<SxProps<Theme> | undefined>) => {
  return props.reduce((acc, curr) => {
    if (!curr) {
      return acc
    }

    if (Array.isArray(curr)) {
      return [...acc, ...curr]
    }

    return [...acc, curr]
  }, [] as SxProps<Theme>[])
}
