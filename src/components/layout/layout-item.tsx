import { useNavigate, type Params, type Path } from "@/router";
import type { LinkProps } from "@generouted/react-router/client";
import { ListItem, ListItemButton, ListItemText, alpha } from "@mui/material";
import { useMatch, useResolvedPath } from "react-router-dom";
export const LayoutItem = (props: LinkProps<Path, Params>) => {
  const { to, children } = props;

  const resolved = useResolvedPath(to);
  const match = useMatch({ path: resolved.pathname, end: true });
  const navigate = useNavigate();

  return (
    <ListItem sx={{ py: 0.5, maxWidth: 250, mx: "auto" }}>
      <ListItemButton
        selected={!!match}
        sx={[
          {
            borderRadius: 8,
            textAlign: "center",
            "& .MuiListItemText-primary": { color: "text.secondary" },
          },
          ({ palette: { mode, primary } }) => {
            const bgcolor =
              mode === "light"
                ? alpha(primary.main, 0.15)
                : alpha(primary.main, 0.35);
            const color = mode === "light" ? primary.main : primary.light;

            return {
              "&.Mui-selected": { bgcolor },
              "&.Mui-selected:hover": { bgcolor },
              "&.Mui-selected .MuiListItemText-primary": { color },
            };
          },
        ]}
        onClick={() => navigate(to)}
      >
        <ListItemText primary={children} />
      </ListItemButton>
    </ListItem>
  );
};
