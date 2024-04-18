import { ListItem, ListItemText, Switch, SwitchProps } from "@mui/material";

interface Props extends SwitchProps {
  label: string;
}

export const SwitchItem = ({ label, ...switchProps }: Props) => {
  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary={label} />

      <Switch {...switchProps} />
    </ListItem>
  );
};
