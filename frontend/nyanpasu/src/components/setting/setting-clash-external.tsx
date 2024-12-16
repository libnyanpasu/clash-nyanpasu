import { ChangeEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useGlobalMutation } from '@/utils/mutation'
import { message } from '@/utils/notification'
import Done from '@mui/icons-material/Done'
import LoadingButton from '@mui/lab/LoadingButton'
import {
  Box,
  List,
  ListItem,
  ListItemText,
  TextField,
  TextFieldProps,
} from '@mui/material'
import { useClash, useNyanpasu } from '@nyanpasu/interface'
import { BaseCard, Expand, MenuItem } from '@nyanpasu/ui'

export const SettingClashExternal = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  const { getClashInfo, setConfigs } = useClash()
  const mutate = useGlobalMutation()

  type PortStrategy = 'fixed' | 'random' | 'allow_fallback'

  const portStrategyOptions = {
    allow_fallback: t('Allow Fallback'),
    fixed: t('Fixed'),
    random: t('Random'),
  }

  const textFieldProps: TextFieldProps = {
    size: 'small',
    variant: 'outlined',
    sx: { width: 160 },
    inputProps: {
      'aria-autocomplete': 'none',
    },
  }

  // What even are these fields?????
  // I had to write the shit code to make it run like a pile of crap.
  const [config, setConfig] = useState({
    portStrategy:
      nyanpasuConfig?.clash_strategy?.external_controller_port_strategy ||
      'allow_fallback',
    controller: getClashInfo.data?.server || '',
    secret: getClashInfo.data?.secret || '',
  })

  useEffect(() => {
    setConfig({
      portStrategy:
        nyanpasuConfig?.clash_strategy?.external_controller_port_strategy ||
        'allow_fallback',
      controller: getClashInfo.data?.server || '',
      secret: getClashInfo.data?.secret || '',
    })
  }, [nyanpasuConfig, getClashInfo.data])

  const [expand, setExpand] = useState(false)

  const [loading, setLoading] = useState(false)

  const apply = async () => {
    setLoading(true)

    try {
      await Promise.all([
        setNyanpasuConfig({
          clash_strategy: {
            external_controller_port_strategy: config.portStrategy,
          },
        }),

        setConfigs({
          'external-controller': config.controller,
          secret: config.secret,
        }),
      ])
    } catch (e) {
      message(JSON.stringify(e), {
        title: t('Error'),
        kind: 'error',
      })
    } finally {
      setExpand(false)

      setTimeout(() => {
        setLoading(false)
        mutate(
          (key) =>
            typeof key === 'string' && key.includes('/getRuntimeConfigYaml'),
        )
      }, 300)
    }
  }

  return (
    <BaseCard label={t('Clash External Controll')}>
      <List disablePadding>
        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={t('External Controller')} />

          <TextField
            value={config.controller}
            onChange={(e: ChangeEvent<HTMLInputElement>) => {
              setConfig((v) => ({ ...v, controller: e.target.value }))
              setExpand(true)
            }}
            {...textFieldProps}
            disabled={loading}
          />
        </ListItem>

        <MenuItem
          label={t('Port Strategy')}
          options={portStrategyOptions}
          selected={config.portStrategy}
          onSelected={(value) => {
            setConfig((v) => ({
              ...v,
              portStrategy: value as PortStrategy,
            }))
            setExpand(true)
          }}
          selectSx={{ width: 160 }}
          disabled={loading}
        />

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={t('Core Secret')} />

          <TextField
            value={config.secret}
            disabled={loading}
            onChange={(e: ChangeEvent<HTMLInputElement>) => {
              setConfig((v) => ({ ...v, secret: e.target.value }))
              setExpand(true)
            }}
            {...textFieldProps}
          />
        </ListItem>

        <Expand open={expand}>
          <Box display="flex" justifyContent="end" alignItems="center" gap={8}>
            <LoadingButton
              loading={loading}
              variant="contained"
              startIcon={<Done />}
              onClick={apply}
            >
              {t('Apply')}
            </LoadingButton>
          </Box>
        </Expand>
      </List>
    </BaseCard>
  )
}

export default SettingClashExternal
