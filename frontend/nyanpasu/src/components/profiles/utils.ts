import { Profile } from "@nyanpasu/interface";

export const filterProfiles = (items?: Profile.Item[]) => {
  const getItems = (types: (string | { script: string })[]) => {
    return items?.filter((i) => {
      if (!i) return false;

      if (typeof i.type === "string") {
        return types.includes(i.type);
      }

      if (typeof i.type === "object" && i.type !== null) {
        return types.some(
          (type) =>
            typeof type === "object" &&
            (i.type as { script: string }).script === type.script,
        );
      }

      return false;
    });
  };

  const profiles = getItems(["local", "remote"]);

  const scripts = getItems([
    "merge",
    { script: "javascript" },
    { script: "lua" },
  ]);

  return {
    profiles,
    scripts,
  };
};
