import { Profile } from "@nyanpasu/interface";

export const filterProfiles = (items?: Profile.Item[]) => {
  const getItems = (types: Profile.Item["type"][]) => {
    return items?.filter((i) => i && types.includes(i.type!));
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
