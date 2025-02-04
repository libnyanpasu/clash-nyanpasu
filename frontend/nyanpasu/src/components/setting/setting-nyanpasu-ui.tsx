import { useAtom } from 'jotai'
import { MuiColorInput } from 'mui-color-input'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isHexColor } from 'validator'
import { defaultTheme } from '@/pages/-theme'
import { atomIsDrawerOnlyIcon } from '@/store'
import { languageOptions } from '@/utils/language'
import Done from '@mui/icons-material/Done'
import { Button, List, ListItem, ListItemText } from '@mui/material'
import { useSetting } from '@nyanpasu/interface'
import { BaseCard, Expand, MenuItem, SwitchItem } from '@nyanpasu/ui'

const commonSx = {
  width: 128,
}

const LanguageSwitch = () => {
  const { t } = useTranslation()

  const language = useSetting('language')

  return (
    <MenuItem
      label={t('Language')}
      selectSx={commonSx}
      options={languageOptions}
      selected={language.value || 'en'}
      onSelected={(value) => language.upsert(value as string)}
    />
  )
}

const ThemeSwitch = () => {
  const { t } = useTranslation()

  const themeOptions = {
    dark: t('theme.dark'),
    light: t('theme.light'),
    system: t('theme.system'),
  }

  const themeMode = useSetting('theme_mode')

  return (
    <MenuItem
      label={t('Theme Mode')}
      selectSx={commonSx}
      options={themeOptions}
      selected={themeMode.value || 'system'}
      onSelected={(value) => themeMode.upsert(value as string)}
    />
  )
}

const ThemeColor = () => {
  const { t } = useTranslation()

  const theme = useSetting('theme_color')

  const [value, setValue] = useState(theme.value ?? defaultTheme.primary_color)

  useEffect(() => {
    setValue(theme.value ?? defaultTheme.primary_color)
  }, [theme.value])

  return (
    <>
      <ListItem sx={{ pl: 0, pr: 0 }}>
        <ListItemText primary={t('Theme Setting')} />

        <MuiColorInput
          size="small"
          sx={commonSx}
          value={theme.value ?? '#1867c0'}
          isAlphaHidden
          format="hex"
          onBlur={() => {
            if (!isHexColor(value ?? defaultTheme.primary_color)) {
              setValue(value)
            }
          }}
          onChange={(color: string) => setValue(color)}
        />
      </ListItem>

      <Expand open={theme.value !== value}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => {
              theme.upsert(value)
            }}
          >
            {t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  )
}

export const SettingNyanpasuUI = () => {
  const { t } = useTranslation()

  const [onlyIcon, setOnlyIcon] = useAtom(atomIsDrawerOnlyIcon)

  return (
    <BaseCard label={t('User Interface')}>
      <List disablePadding>
        <LanguageSwitch />

        <ThemeSwitch />

        <ThemeColor />

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
