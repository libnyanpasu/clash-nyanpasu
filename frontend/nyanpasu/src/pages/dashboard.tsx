import { useTranslation } from "react-i18next";
import DataPanel from "@/components/dashboard/data-panel";
import HealthPanel from "@/components/dashboard/health-panel";
import Grid from "@mui/material/Unstable_Grid2";
import { BasePage } from "@nyanpasu/ui";

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
