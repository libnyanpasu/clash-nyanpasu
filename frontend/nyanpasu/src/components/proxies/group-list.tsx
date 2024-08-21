import { useAtom } from "jotai";
import { memo, RefObject, useMemo } from "react";
import useSWR from "swr";
import { Virtualizer } from "virtua";
import { proxyGroupAtom } from "@/store";
import {
  alpha,
  ListItem,
  ListItemButton,
  ListItemButtonProps,
  ListItemIcon,
  ListItemText,
  useTheme,
} from "@mui/material";
import { getServerPort, useClashCore } from "@nyanpasu/interface";
import { LazyImage } from "@nyanpasu/ui";

const IconRender = memo(function IconRender({ icon }: { icon: string }) {
  const {
    data: serverPort,
    isLoading,
    error,
  } = useSWR("/getServerPort", getServerPort);
  const src = icon.trim().startsWith("<svg")
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon;
  const cachedUrl = useMemo(() => {
    if (!src.startsWith("http")) {
      return src;
    }
    return `http://localhost:${serverPort}/cache/icon?url=${btoa(src)}`;
  }, [src, serverPort]);
  if (isLoading || error) {
    return null;
  }
  return (
    <ListItemIcon>
      <LazyImage
        className="h-11 w-11"
        loadingClassName="rounded-full"
        src={cachedUrl}
      />
    </ListItemIcon>
  );
});

export interface GroupListProps extends ListItemButtonProps {
  scrollRef: RefObject<HTMLElement>;
}

export const GroupList = ({
  scrollRef,
  ...listItemButtonProps
}: GroupListProps) => {
  const { data } = useClashCore();

  const { palette } = useTheme();

  const [proxyGroup, setProxyGroup] = useAtom(proxyGroupAtom);

  const handleSelect = (index: number) => {
    setProxyGroup({ selector: index });
  };

  return (
    <Virtualizer scrollRef={scrollRef}>
      {data?.groups?.map((group, index) => {
        const selected = index === proxyGroup.selector;

        return (
          <ListItem key={index} disablePadding>
            <ListItemButton
              selected={selected}
              onClick={() => handleSelect(index)}
              sx={{
                backgroundColor: selected
                  ? `${alpha(palette.primary.main, 0.3)} !important`
                  : undefined,
              }}
              {...listItemButtonProps}
            >
              {group.icon && <IconRender icon={group.icon} />}

              <ListItemText
                className="!truncate"
                primary={group.name}
                secondary={group.now}
              />
            </ListItemButton>
          </ListItem>
        );
      })}
    </Virtualizer>
  );
};
