import { proxyGroupAtom } from "@/store";
import {
  List,
  ListItem,
  ListItemButton,
  ListItemButtonProps,
  ListItemText,
} from "@mui/material";
import { useClashCore } from "@nyanpasu/interface";
import { useAtom } from "jotai";

export const GroupList = (listItemButtonProps: ListItemButtonProps) => {
  const { data } = useClashCore();

  const [proxyGroup, setProxyGroup] = useAtom(proxyGroupAtom);

  const handleSelect = (index: number) => {
    setProxyGroup({ selector: index });
  };

  return (
    <List disablePadding>
      {data?.groups?.map((group, index) => {
        return (
          <ListItem key={index} disablePadding>
            <ListItemButton
              selected={index === proxyGroup.selector}
              onClick={() => handleSelect(index)}
              {...listItemButtonProps}
            >
              <ListItemText primary={group.name} secondary={group.now} />
            </ListItemButton>
          </ListItem>
        );
      })}
    </List>
  );
};
