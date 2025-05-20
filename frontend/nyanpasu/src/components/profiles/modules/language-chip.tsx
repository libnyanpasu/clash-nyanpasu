import { Box } from '@mui/material'
import { alpha } from '@nyanpasu/ui'
import { getLanguage, ProfileType } from '../utils'

export const LanguageChip = ({ type }: { type: ProfileType }) => {
  const lang = getLanguage(type, true)

  return (
    lang && (
      <Box
        className="my-auto rounded-full px-2 py-0.5 text-center text-sm font-bold"
        sx={(theme) => ({
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
          color: theme.vars.palette.primary.main,
        })}
      >
        {lang}
      </Box>
    )
  )
}

export default LanguageChip
