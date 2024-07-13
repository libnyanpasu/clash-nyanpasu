import { BasePage } from "@nyanpasu/ui";
import Grid from "@mui/material/Unstable_Grid2";
import DataPanel from "@/components/dashboard/data-panel";
import { useTranslation } from "react-i18next";
import HealthPanel from "@/components/dashboard/health-panel";

export const Dashboard = () => {
  const { t } = useTranslation();

  return (
    <BasePage title={t("Dashboard")}>
      <Grid container spacing={2} sx={{ width: "calc(100% + 24px)" }}>
        <DataPanel />

        <HealthPanel />
      </Grid>
    </BasePage>
  );
};

export default Dashboard;
