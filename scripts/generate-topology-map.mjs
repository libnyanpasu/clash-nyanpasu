import { format } from "prettier";
import { mkdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(
  new URL("../frontend/nyanpasu/package.json", import.meta.url),
);
const { geoNaturalEarth1, geoPath, geoGraticule10 } = await import(
  pathToFileURL(require.resolve("d3")).href
);
const revision = "ca96624a56bd078437bca8184e78163e5039ad19";
const source =
  `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/${revision}/geojson`;
const [countries, locations] = await Promise.all(
  [110, 10].map(async (scale) => {
    const response = await fetch(
      `${source}/ne_${scale}m_admin_0_countries.geojson`,
    );
    if (!response.ok) {
      throw new Error(`Natural Earth download failed: ${response.status}`);
    }
    return response.json();
  }),
);
const projection = geoNaturalEarth1().fitExtent([[18, 18], [942, 442]], {
  type: "Sphere",
});
const path = geoPath(projection).digits(1);
const centers = {};
for (const { properties: p } of locations.features) {
  const code = p.ISO_A2_EH;
  if (!/^[A-Z]{2}$/.test(code)) continue;
  centers[code] = projection([p.LABEL_X, p.LABEL_Y]).map((value) =>
    Math.round(value * 10) / 10
  );
}
const output = new URL(
  "../frontend/nyanpasu/src/assets/maps/world-map.json",
  import.meta.url,
);
await mkdir(fileURLToPath(new URL(".", output)), { recursive: true });
await writeFile(
  output,
  await format(
    JSON.stringify({
      outline: path(countries),
      graticule: path(geoGraticule10()),
      centers: Object.fromEntries(Object.entries(centers).sort()),
    }),
    { parser: "json" },
  ),
);
console.log(
  `Generated offline map with ${Object.keys(centers).length} region labels`,
);
