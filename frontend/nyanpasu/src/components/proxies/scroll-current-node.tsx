import { Radar } from "@mui/icons-material";
import { Button, Tooltip, alpha, useTheme } from "@mui/material";
import { useTranslation } from "react-i18next";

export const ScrollCurrentNode = ({ onClick }: { onClick?: () => void }) => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  return (
    <Tooltip title={t("Location")}>
      <Button
        size="small"
        className="!min-w-0 !size-8"
        sx={{
          backgroundColor: alpha(palette.primary.main, 0.1),
        }}
        onClick={onClick}
      >
        <Radar />
      </Button>
    </Tooltip>
  );
};

export default ScrollCurrentNode;
