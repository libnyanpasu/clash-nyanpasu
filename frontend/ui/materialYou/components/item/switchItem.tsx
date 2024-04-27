import { Switch, SwitchProps } from "@mui/material";
import { BaseItem } from "./baseItem";

interface Props extends SwitchProps {
  label: string;
}

export const SwitchItem = ({ label, ...switchProps }: Props) => {
  return (
    <BaseItem title={label}>
      <Switch {...switchProps} />
    </BaseItem>
  );
};
