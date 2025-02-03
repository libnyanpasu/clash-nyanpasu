// nyanpasu merge profile chain template
const merge = `# Clash Nyanpasu Merge Template (YAML)
# Documentation on https://nyanpasu.elaina.moe/
# Set the default merge strategy to recursive merge. 
# Enable the old mode with the override__ prefix. 
# Use the filter__ prefix to filter lists (removing unwanted content). 
# All prefixes should support accessing maps or lists with a.b.c syntax.
`

// nyanpasu javascript profile chain template
const javascript = `// Clash Nyanpasu JavaScript Template
// Documentation on https://nyanpasu.elaina.moe/

/** @type {config} */
export default function (profile) {
return profile;
}
`

// nyanpasu lua profile chain template
const luascript = `-- Clash Nyanpasu Lua Script Template
-- Documentation on https://nyanpasu.elaina.moe/

return config;
`

// clash profile template example
const profile = `# Clash Nyanpasu Profile Template
# Documentation on https://nyanpasu.elaina.moe/

proxies:

proxy-groups:

rules:
`

export const ProfileTemplate = {
  merge,
  javascript,
  luascript,
  profile,
} as const
