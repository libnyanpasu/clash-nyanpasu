import { useNyanpasu } from "@nyanpasu/interface";
import { AnimatePresence, motion } from "framer-motion";
import { memo } from "react";

export const ProxyGroupName = memo(function ProxyGroupName({
  name,
}: {
  name: string;
}) {
  const { nyanpasuConfig } = useNyanpasu();

  const disbaleMotion = nyanpasuConfig?.lighten_animation_effects;

  return disbaleMotion ? (
    <>{name}</>
  ) : (
    <AnimatePresence mode="sync" initial={false}>
      <motion.div
        key={`group-name-${name}`}
        className="absolute"
        initial={{ x: 100, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        exit={{ x: -100, opacity: 0 }}
      >
        {name}
      </motion.div>
    </AnimatePresence>
  );
});

export default ProxyGroupName;
