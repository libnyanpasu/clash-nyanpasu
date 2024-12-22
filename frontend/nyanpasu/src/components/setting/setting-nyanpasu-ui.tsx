import { useAtom } from 'jotai'
import { MuiColorInput } from 'mui-color-input'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isHexColor } from 'validator'
import { defaultTheme } from '@/pages/-theme'
import { atomIsDrawerOnlyIcon } from '@/store'
import { languageOptions } from '@/utils/language'
import Done from '@mui/icons-material/Done'
import { Box, Button, List, ListItem, ListItemText } from '@mui/material'
import { useNyanpasu, VergeConfig } from '@nyanpasu/interface'
import { BaseCard, Expand, MenuItem, SwitchItem } from '@nyanpasu/ui'

export const SettingNyanpasuUI = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  const themeOptions = {
    dark: t('theme.dark'),
    light: t('theme.light'),
    system: t('theme.system'),
  }

  const [themeColor, setThemeColor] = useState(
    nyanpasuConfig?.theme_setting?.primary_color,
  )
  const themeColorRef = useRef(themeColor)

  const commonSx = {
    width: 128,
  }

  const [onlyIcon, setOnlyIcon] = useAtom(atomIsDrawerOnlyIcon)

  return (
    <BaseCard label={t('User Interface')}>
      <List disablePadding>
        <MenuItem
          label={t('Language')}
          selectSx={commonSx}
          options={languageOptions}
          selected={nyanpasuConfig?.language || 'en'}
          onSelected={(value) =>
            setNyanpasuConfig({ language: value as string })
          }
        />

        <MenuItem
          label={t('Theme Mode')}
          selectSx={commonSx}
          options={themeOptions}
          selected={nyanpasuConfig?.theme_mode || 'light'}
          onSelected={(value) =>
            setNyanpasuConfig({
              theme_mode: value as VergeConfig['theme_mode'],
            })
          }
        />

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={t('Theme Setting')} />

          <MuiColorInput
            size="small"
            sx={commonSx}
            value={themeColor ?? defaultTheme.primary_color}
            isAlphaHidden
            format="hex"
            onBlur={() => {
              if (
                !isHexColor(themeColorRef.current ?? defaultTheme.primary_color)
              ) {
                setThemeColor(themeColorRef.current)
                return
              }
              themeColorRef.current = themeColor
            }}
            onChange={(color: string) => setThemeColor(color)}
          />
        </ListItem>

        <Expand
          open={nyanpasuConfig?.theme_setting?.primary_color !== themeColor}
        >
          <Box
            sx={{ pb: 1 }}
            display="flex"
            justifyContent="end"
            alignItems="center"
          >
            <Button
              variant="contained"
              startIcon={<Done />}
              onClick={() => {
                setNyanpasuConfig({
                  theme_setting: {
                    ...nyanpasuConfig?.theme_setting,
                    primary_color: themeColor,
                  },
                })
              }}
            >
              Apply
            </Button>
          </Box>
        </Expand>

        <SwitchItem
          label={t('Icon Navigation Bar')}
          checked={onlyIcon}
          onChange={() => setOnlyIcon(!onlyIcon)}
        />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuUI
