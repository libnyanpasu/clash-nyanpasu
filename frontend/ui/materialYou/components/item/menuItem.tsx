import {
  ListItem,
  ListItemText,
  MenuItem as MuiMenuItem,
  Select,
} from "@mui/material";

type OptionValue = string | number | boolean;

export interface MenuItemProps {
  label: string;
  options: Record<string, OptionValue>;
  selected: OptionValue;
  onSelected: (value: OptionValue) => void;
}

export const MenuItem = ({
  label,
  options,
  selected,
  onSelected,
}: MenuItemProps) => {
  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary={label} />

      <Select
        size="small"
        value={selected}
        inputProps={{ "aria-label": "Without label" }}
        onChange={(e) => {
          onSelected(e.target.value);
        }}
      >
        {Object.entries(options).map(([key, value]) => (
          <MuiMenuItem
            key={key}
            value={key}
            disabled={key === selected}
            selected={key === selected}
          >
            {value}
          </MuiMenuItem>
        ))}
      </Select>
    </ListItem>
  );
};

export default MenuItem;
