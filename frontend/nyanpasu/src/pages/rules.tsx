import { BaseEmpty } from "@/components/base";
import RuleItem from "@/components/rules/rule-item";
import { alpha, FilledInputProps, TextField, useTheme } from "@mui/material";
import { useClashCore } from "@nyanpasu/interface";
import { BasePage } from "@nyanpasu/ui";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { VList } from "virtua";

export default function RulesPage() {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const { getRules } = useClashCore();

  const [filterText, setFilterText] = useState("");

  const rules = useMemo(() => {
    return getRules.data?.rules.filter((each) =>
      each.payload.includes(filterText),
    );
  }, [getRules.data, filterText]);

  const inputProps: Partial<FilledInputProps> = {
    sx: {
      borderRadius: 7,
      backgroundColor: alpha(palette.primary.main, 0.1),

      fieldset: {
        border: "none",
      },
    },
  };

  return (
    <BasePage
      full
      title={t("Rules")}
      contentStyle={{ height: "100%" }}
      header={
        <TextField
          hiddenLabel
          autoComplete="off"
          spellCheck="false"
          value={filterText}
          placeholder={t("Filter conditions")}
          onChange={(e) => setFilterText(e.target.value)}
          className="!pb-0"
          sx={{ input: { py: 1, fontSize: 14 } }}
          InputProps={inputProps}
        />
      }
    >
      <VList className="flex flex-col gap-2 p-2 overflow-auto select-text">
        {rules ? (
          rules.map((item, index) => {
            return <RuleItem key={index} index={index} value={item} />;
          })
        ) : (
          <BaseEmpty text="No Rules" />
        )}
      </VList>
    </BasePage>
  );
}
