import {
  alpha,
  Box,
  ListItemText,
  ListItemButton,
  Typography,
  styled,
  Paper,
  Divider,
} from "@mui/material";
import {
  ExpandLessRounded,
  ExpandMoreRounded,
  InboxRounded,
} from "@mui/icons-material";
import { HeadState } from "./use-head-state";
import { ProxyHead } from "./proxy-head";
import { ProxyItem } from "./proxy-item";
import { ProxyItemMini } from "./proxy-item-mini";
import type { IRenderItem } from "./use-render-list";

interface RenderProps {
  item: IRenderItem;
  indent: boolean;
  onLocation: (group: IProxyGroupItem) => void;
  onCheckAll: (groupName: string) => void;
  onHeadState: (groupName: string, patch: Partial<HeadState>) => void;
  onChangeProxy: (group: IProxyGroupItem, proxy: IProxyItem) => void;
}

export const ProxyRender = (props: RenderProps) => {
  const { indent, item, onLocation, onCheckAll, onHeadState, onChangeProxy } =
    props;
  const { type, group, headState, proxy, proxyCol } = item;

  if (type === 0) {
    return (
      <Paper
        sx={{
          boxShadow: 2,
          borderRadius: 7,
          marginTop: 2,
          marginBottom: 2,
        }}
      >
        <ListItemButton
          dense
          onClick={() => onHeadState(group!.name, { open: !headState?.open })}
          sx={{
            borderRadius: 7,
          }}
        >
          <ListItemText
            primary={group!.name}
            secondary={
              <ListItemTextChild
                sx={{
                  overflow: "hidden",
                  display: "flex",
                  alignItems: "center",
                  pt: "2px",
                }}
              >
                <StyledTypeBox>{group!.type}</StyledTypeBox>
                <StyledSubtitle>{group!.now}</StyledSubtitle>
              </ListItemTextChild>
            }
            secondaryTypographyProps={{
              sx: { display: "flex", alignItems: "center" },
            }}
          />
          {headState?.open ? <ExpandLessRounded /> : <ExpandMoreRounded />}
        </ListItemButton>
      </Paper>
    );
  }

  if (type === 1) {
    return (
      <Paper
        sx={{
          boxShadow: 2,
          borderTopLeftRadius: "28px",
          borderTopRightRadius: "28px",
          borderBottomLeftRadius: "0",
          borderBottomRightRadius: "0",
        }}
      >
        <ProxyHead
          sx={{
            pl: 2,
            pr: 2,
            pt: 1,
            pb: 1,
            mt: indent ? 1 : 0.5,
          }}
          groupName={group!.name}
          headState={headState!}
          onLocation={() => onLocation(group!)}
          onCheckDelay={() => onCheckAll(group!.name)}
          onHeadState={(p) => onHeadState(group!.name, p)}
        />

        <Divider />
      </Paper>
    );
  }

  if (type === 2) {
    return (
      <Paper
        sx={{
          boxShadow:
            "0px 1px 1px -2px rgba(0,0,0,0.2), 0px 2px 2px 0px rgba(0,0,0,0.14), 0px 5px 5px 0px rgba(0,0,0,0.12)",
          borderRadius: "0",
        }}
      >
        <ProxyItem
          groupName={group!.name}
          proxy={proxy!}
          selected={group!.now === proxy?.name}
          showType={headState?.showType}
          sx={{ py: 0, pl: 2, pb: 1 }}
          onClick={() => onChangeProxy(group!, proxy!)}
        />
      </Paper>
    );
  }

  if (type === 3) {
    return (
      <Box
        sx={{
          py: 2,
          pl: indent ? 4.5 : 0,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <InboxRounded sx={{ fontSize: "2.5em", color: "inherit" }} />
        <Typography sx={{ color: "inherit" }}>No Proxies</Typography>
      </Box>
    );
  }

  if (type === 4) {
    return (
      <Paper
        sx={{
          boxShadow:
            "0px 1px 1px -2px rgba(0,0,0,0.2), 0px 2px 2px 0px rgba(0,0,0,0.14), 0px 5px 5px 0px rgba(0,0,0,0.12)",
          borderRadius: "0",
        }}
      >
        <Box
          sx={{
            height: 56,
            display: "grid",
            gap: 1.5,
            pl: 2,
            pr: 2,
            pt: 1,
            pb: 1,
            gridTemplateColumns: `repeat(${item.col! || 2}, 1fr)`,
          }}
        >
          {proxyCol?.map((proxy) => (
            <ProxyItemMini
              key={item.key + proxy.name}
              groupName={group!.name}
              proxy={proxy!}
              selected={group!.now === proxy.name}
              showType={headState?.showType}
              onClick={() => onChangeProxy(group!, proxy!)}
            />
          ))}
        </Box>
      </Paper>
    );
  }

  if (type === 5) {
    return (
      <Paper
        sx={{
          boxShadow:
            "0px 1px 1px -2px rgba(0,0,0,0.2), 0px 2px 2px 0px rgba(0,0,0,0.14), 0px 5px 5px 0px rgba(0,0,0,0.12)",
          borderTopLeftRadius: "0",
          borderTopRightRadius: "0",
          borderBottomLeftRadius: "28px",
          borderBottomRightRadius: "28px",
          height: 28,
        }}
      >
        <Divider />
      </Paper>
    );
  }

  if (type === 6) {
    return (
      <Box
        sx={{
          height: 28,
        }}
      />
    );
  }

  return null;
};

const StyledSubtitle = styled("span")`
  font-size: 0.8rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`;

const ListItemTextChild = styled("span")`
  display: block;
`;

const StyledTypeBox = styled(ListItemTextChild)(({ theme }) => ({
  display: "inline-block",
  border: "1px solid #ccc",
  borderColor: alpha(theme.palette.primary.main, 0.5),
  color: alpha(theme.palette.primary.main, 0.8),
  borderRadius: 4,
  fontSize: 10,
  padding: "0 2px",
  lineHeight: 1.25,
  marginRight: "4px",
}));
