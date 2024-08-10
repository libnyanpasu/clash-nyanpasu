import { motion } from "framer-motion";
import { ComponentType, lazy, Suspense, useState } from "react";
import { Awaitable, cn } from "@/utils";
import LinearProgress from "@mui/material/LinearProgress";

export interface AsyncComponentProps {
  component: () => Awaitable<{ default: ComponentType }>;
}

export const AsyncComponent = ({ component }: AsyncComponentProps) => {
  const [isLoaded, setIsLoaded] = useState(false);

  const Component = lazy(async () => {
    const module = await component();
    setIsLoaded(true);
    return module;
  });

  return (
    <Suspense
      fallback={
        <div
          className={cn(
            "absolute flex h-full w-full flex-col items-center justify-center transition-opacity",
            isLoaded ? "opacity-0" : "opacity-100",
          )}
        >
          <LinearProgress className="w-40" />
        </div>
      }
    >
      <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
        <Component />
      </motion.div>
    </Suspense>
  );
};
