import { SwitchProps } from "@mui/material/Switch/Switch";
import { Clash, useClash, useNyanpasu, VergeConfig } from "@nyanpasu/interface";
import { MenuItemProps } from "@nyanpasu/ui";

type OptionValue = string | number | boolean;

interface CreateMenuPropsOptions {
  options: Record<string, OptionValue>;
  fallbackSelect: OptionValue;
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
  createBooleanProps: (
    propName: {
      [K in keyof Clash.Config]: Clash.Config[K] extends boolean ? K : never;
    }[keyof Clash.Config],
  ): SwitchProps => {
    const { getConfigs, setConfigs } = useClash();

    return {
      checked: getConfigs.data?.[propName] || false,
      onChange: () => {
        setConfigs({ [propName]: !getConfigs.data?.[propName] });
      },
    };
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
  createMenuProps: (
    propName: keyof Clash.Config,
    { options, fallbackSelect }: CreateMenuPropsOptions,
  ): Omit<MenuItemProps, "label"> => {
    const { getConfigs, setConfigs } = useClash();

    return {
      options,
      selected: getConfigs.data?.[propName] || fallbackSelect,
      onSelected: (value: OptionValue) => {
        setConfigs({ [propName]: value });
      },
    };
  },
};

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
  createBooleanProps: (propName: keyof VergeConfig): SwitchProps => {
    const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu();

    if (typeof nyanpasuConfig?.[propName] !== "boolean") {
      throw new Error(`Property ${propName} is not a boolean type`);
    }

    return {
      checked: (nyanpasuConfig?.[propName] as boolean) || false,
      onChange: async () => {
        await setNyanpasuConfig({ [propName]: !nyanpasuConfig?.[propName] });
      },
    };
  },
};
