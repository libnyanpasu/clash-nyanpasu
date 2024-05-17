import { Button, alpha, useTheme } from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";
import Grid from "@mui/material/Unstable_Grid2";
import { Add } from "@mui/icons-material";
import ProfileItem from "@/components/profiles/profile-item";
import { filterProfiles } from "@/components/profiles/utils";

export const ProfilePage = () => {
  const { t } = useTranslation();

  const { getProfiles } = useNyanpasu();

  const { profiles } = filterProfiles(getProfiles.data?.items);

  const { palette } = useTheme();

  return (
    <SidePage title={t("Profiles")} flexReverse>
      <div className="p-6">
        <Grid container spacing={2}>
          {profiles?.map((item, index) => {
            return (
              <Grid key={index} xs={12} sm={6} md={6} xl={4}>
                <ProfileItem item={item} />
              </Grid>
            );
          })}
        </Grid>
      </div>

      <Button
        className="size-16 backdrop-blur !rounded-2xl !absolute z-10 bottom-8 right-8"
        sx={{
          boxShadow: 8,
          backgroundColor: alpha(palette.primary.main, 0.3),

          "&:hover": {
            backgroundColor: alpha(palette.primary.main, 0.45),
          },
        }}
      >
        <Add className="!size-8 absolute" />
      </Button>
    </SidePage>
  );
};

export default ProfilePage;
