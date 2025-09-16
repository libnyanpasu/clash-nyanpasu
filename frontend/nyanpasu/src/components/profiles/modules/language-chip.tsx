import { Box } from '@mui/material'
import { alpha } from '@nyanpasu/ui'

export const LanguageChip = ({ lang }: { lang: string }) => {
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
