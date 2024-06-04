import { Close } from "@mui/icons-material";
import { Tooltip } from "@mui/material";
import { useClash } from "@nyanpasu/interface";
import { FloatingButton } from "@nyanpasu/ui";
import { useLockFn } from "ahooks";
import { useTranslation } from "react-i18next";

export const CloseConnectionsButton = () => {
  const { t } = useTranslation();

  const { deleteConnections } = useClash();

  const onCloseAll = useLockFn(async () => {
    await deleteConnections();
  });

  return (
    <Tooltip title={t("Close All")}>
      <FloatingButton onClick={onCloseAll}>
        <Close className="!size-8 absolute" />
      </FloatingButton>
    </Tooltip>
  );
};

export default CloseConnectionsButton;
