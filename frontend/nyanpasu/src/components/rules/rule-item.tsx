import { Box, SxProps, Theme } from '@mui/material'
import { ClashRule } from '@nyanpasu/interface'

interface Props {
  index: number
  value: ClashRule
}

const COLOR = [
  (theme) => ({
    color: theme.vars.palette.primary.main,
  }),
  (theme) => ({
    color: theme.vars.palette.secondary.main,
  }),
  (theme) => ({
    color: theme.vars.palette.info.main,
  }),
  (theme) => ({
    color: theme.vars.palette.warning.main,
  }),
  (theme) => ({
    color: theme.vars.palette.success.main,
  }),
] satisfies SxProps<Theme>[]

const RuleItem = ({ index, value }: Props) => {
  const parseColorSx: (text: string) => SxProps<Theme> = (text) => {
    const TYPE = {
      reject: ['REJECT', 'REJECT-DROP'],
      direct: ['DIRECT'],
    }

    if (TYPE.reject.includes(text))
      return (theme) => ({ color: theme.vars.palette.error.main })

    if (TYPE.direct.includes(text))
      return (theme) => ({ color: theme.vars.palette.text.primary })

    let sum = 0

    for (let i = 0; i < text.length; i++) {
      sum += text.charCodeAt(i)
    }

    return COLOR[sum % COLOR.length]
  }

  return (
    <div className="flex p-2 pr-7 pl-7 select-text">
      <Box
        sx={(theme) => ({ color: theme.vars.palette.text.secondary })}
        className="min-w-14"
      >
        {index + 1}
      </Box>

      <div className="flex flex-col gap-1">
        <Box sx={(theme) => ({ color: theme.vars.palette.text.primary })}>
          {value.payload || '-'}
        </Box>

        <div className="flex gap-8">
          <div className="min-w-40 text-sm">{value.type}</div>

          <Box className="text-s text-sm" sx={parseColorSx(value.proxy)}>
            {value.proxy}
          </Box>
        </div>
      </div>
    </div>
  )
}

export default RuleItem
