import { SwitchProps } from "@mui/material/Switch/Switch";
import { Clash, useClash } from "@nyanpasu/interface";
import { MenuItemProps } from "@nyanpasu/ui";

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
export const createBooleanProps = (
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
};

type OptionValue = string | number | boolean;

interface CreateMenuPropsOptions {
  options: Record<string, OptionValue>;
  fallbackSelect: OptionValue;
}

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
export const createMenuProps = (
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
};
