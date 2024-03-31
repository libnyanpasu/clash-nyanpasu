import { BaseEmpty, BasePage } from "@/components/base";
import RuleItem from "@/components/rule/rule-item";
import { getRules } from "@/services/api";
import { Box, Paper, TextField } from "@mui/material";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Virtuoso } from "react-virtuoso";
import useSWR from "swr";

export default function RulesPage() {
  const { t } = useTranslation();
  const { data = [] } = useSWR("getRules", getRules);

  const [filterText, setFilterText] = useState("");

  const rules = useMemo(() => {
    return data.filter((each) => each.payload.includes(filterText));
  }, [data, filterText]);

  return (
    <BasePage full title={t("Rules")} contentStyle={{ height: "100%" }}>
      <Box
        sx={{
          padding: 2,
          width: "calc(100% - 32px)",
          position: "fixed",
          borderRadius: 4,
          zIndex: 10,
        }}
      >
        <Paper
          sx={{
            borderRadius: 4,
            boxShadow: "none",
          }}
        >
          <TextField
            hiddenLabel
            fullWidth
            size="small"
            autoComplete="off"
            variant="outlined"
            spellCheck="false"
            placeholder={t("Filter conditions")}
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
            sx={{ input: { py: 0.65, px: 1.25 } }}
            InputProps={{
              sx: {
                borderRadius: 4,
              },
            }}
          />
        </Paper>
      </Box>

      <Box height="100%">
        {rules.length > 0 ? (
          <Virtuoso
            data={rules}
            itemContent={(index, item) => (
              <RuleItem index={index} value={item} />
            )}
            followOutput={"smooth"}
            overscan={900}
          />
        ) : (
          <BaseEmpty text="No Rules" />
        )}
      </Box>
    </BasePage>
  );
}
