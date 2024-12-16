import { useGlobalMutation } from '@/utils/mutation'
import { SwitchProps } from '@mui/material/Switch/Switch'
import { Clash, useClash, useNyanpasu, VergeConfig } from '@nyanpasu/interface'
import { MenuItemProps } from '@nyanpasu/ui'

type OptionValue = string | number | boolean

interface CreateMenuPropsOptions {
  options: Record<string, OptionValue>
  fallbackSelect: OptionValue
}

export const clash = {
  /**
   * @example
   * createBooleanProps("ipv6")
   *
   * @returns {SwitchProps}
   * SwitchProps
   *
   * `Only supports boolean-type value keys.`
   *
   * @author keiko233 <i@elaina.moe>
   * @copyright LibNyanpasu org. 2024
   */
  useBooleanProps: (
    propName: {
      [K in keyof Clash.Config]: Clash.Config[K] extends boolean ? K : never
    }[keyof Clash.Config],
  ): SwitchProps => {
    const { getConfigs, setConfigs } = useClash()
    const mutate = useGlobalMutation()
    return {
      checked: getConfigs.data?.[propName] || false,
      onChange: () => {
        Promise.all([
          setConfigs({ [propName]: !getConfigs.data?.[propName] }),
          setConfigs({ [propName]: !getConfigs.data?.[propName] }),
        ]).finally(() => {
          mutate(
            (key) =>
              typeof key === 'string' && key.includes('/getRuntimeConfigYaml'),
          )
        })
      },
    }
  },

  /**
   * @example
   * createMenuProps("log-level", {
      options,
      fallbackSelect: "debug",
    })
   *
   * @returns {Omit<MenuItemProps, "label">}
   * Omit<MenuItemProps, "label">
   *
   * `Recommend use MemuItem component.`
   *
   * @author keiko233 <i@elaina.moe>
   * @copyright LibNyanpasu org. 2024
   */
  useMenuProps: (
    propName: keyof Clash.Config,
    { options, fallbackSelect }: CreateMenuPropsOptions,
  ): Omit<MenuItemProps, 'label'> => {
    const { getConfigs, setConfigs } = useClash()
    const mutate = useGlobalMutation()

    return {
      options,
      selected: getConfigs.data?.[propName] || fallbackSelect,
      onSelected: (value: OptionValue) => {
        Promise.all([setConfigs({ [propName]: value })]).finally(() => {
          mutate(
            (key) =>
              typeof key === 'string' && key.includes('/getRuntimeConfigYaml'),
          )
        })
      },
    }
  },
}

export const nyanpasu = {
  /**
   * @example
   * createBooleanProps("ipv6")
   *
   * @returns {SwitchProps}
   * SwitchProps
   *
   * `Only supports boolean-type value keys.`
   *
   * @author keiko233 <i@elaina.moe>
   * @copyright LibNyanpasu org. 2024
   */
  useBooleanProps: (propName: keyof VergeConfig): SwitchProps => {
    const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

    if (typeof nyanpasuConfig?.[propName] !== 'boolean') {
      throw new Error(`Property ${propName} is not a boolean type`)
    }

    return {
      checked: (nyanpasuConfig?.[propName] as boolean) || false,
      onChange: async () => {
        await setNyanpasuConfig({ [propName]: !nyanpasuConfig?.[propName] })
      },
    }
  },
}
