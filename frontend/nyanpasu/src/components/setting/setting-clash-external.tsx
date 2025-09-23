import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { sleep } from '@/utils'
import Done from '@mui/icons-material/Done'
import { List, ListItem, ListItemText, TextField } from '@mui/material'
import {
  ExternalControllerPortStrategy,
  useClashConfig,
  useClashInfo,
  useRuntimeProfile,
  useSetting,
} from '@nyanpasu/interface'
import {
  BaseCard,
  MUIButton as Button,
  Expand,
  MenuItem,
  TextItemProps,
} from '@nyanpasu/ui'

const TextItem = ({
  value,
  label,
  onApply,
  applyLabel,
  placeholder,
}: TextItemProps) => {
  const { t } = useTranslation()

  const [textString, setTextString] = useState(value)

  useEffect(() => {
    setTextString(value)
  }, [value])

  return (
    <>
      <ListItem sx={{ pl: 0, pr: 0 }}>
        <ListItemText primary={label} />

        <TextField
          value={textString}
          onChange={(e) => setTextString(e.target.value)}
          placeholder={placeholder}
          size="small"
          variant="outlined"
          sx={{ width: 160 }}
          inputProps={{
            'aria-autocomplete': 'none',
          }}
        />
      </ListItem>

      <Expand open={textString !== value}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => onApply(textString)}
          >
            {applyLabel ?? t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  )
}

const ExternalController = () => {
  const { t } = useTranslation()

  const { data, refetch } = useClashInfo()

  const { upsert } = useClashConfig()

  const runtimeProfile = useRuntimeProfile()

  return (
    <TextItem
      label={t('External Controller')}
      value={data?.server || ''}
      onApply={async (value) => {
        await upsert.mutateAsync({ 'external-controller': value })
        await refetch()

        // Wait for the server to apply
        await sleep(300)
        await runtimeProfile.refetch()
      }}
    />
  )
}

const PortStrategy = () => {
  const { t } = useTranslation()

  const portStrategyOptions = {
    allow_fallback: t('Allow Fallback'),
    fixed: t('Fixed'),
    random: t('Random'),
  }

  const { value, upsert } = useSetting('clash_strategy')

  const selected = useMemo(
    () => value?.external_controller_port_strategy || 'allow_fallback',
    [value],
  )

  return (
    <MenuItem
      label={t('Port Strategy')}
      options={portStrategyOptions}
      selected={selected}
      onSelected={async (value) => {
        await upsert({
          external_controller_port_strategy:
            value as ExternalControllerPortStrategy,
        })
      }}
      selectSx={{ width: 160 }}
    />
  )
}

const CoreSecret = () => {
  const { t } = useTranslation()

  const { data, refetch } = useClashInfo()

  const { upsert } = useClashConfig()

  return (
    <TextItem
      label={t('Core Secret')}
      value={data?.secret || ''}
      onApply={async (value) => {
        await upsert.mutateAsync({ secret: value })
        await refetch()
      }}
    />
  )
}

export const SettingClashExternal = () => {
  const { t } = useTranslation()

  return (
    <BaseCard label={t('Clash External Controll')}>
      <List disablePadding>
        <ExternalController />

        <PortStrategy />

        <CoreSecret />
      </List>
    </BaseCard>
  )
}

export default SettingClashExternal
