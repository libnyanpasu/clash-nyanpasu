import { useAtomValue } from "jotai";
import { Virtualizer } from "virtua";
import ContentDisplay from "../base/content-display";
import { atomRulePage } from "./modules/store";
import RuleItem from "./rule-item";

export const RulePage = () => {
  const rule = useAtomValue(atomRulePage);

  return rule?.data?.length ? (
    <Virtualizer scrollRef={rule?.scrollRef}>
      {rule.data.map((item, index) => {
        return <RuleItem key={index} index={index} value={item} />;
      })}
    </Virtualizer>
  ) : (
    <ContentDisplay className="absolute" message="No logs" />
  );
};

export default RulePage;
