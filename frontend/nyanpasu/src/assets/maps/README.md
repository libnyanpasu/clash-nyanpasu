# Offline topology map

`world-map.json` is generated from Natural Earth at commit
`ca96624a56bd078437bca8184e78163e5039ad19`:

- [1:110m outlines](https://github.com/nvkelso/natural-earth-vector/blob/ca96624a56bd078437bca8184e78163e5039ad19/geojson/ne_110m_admin_0_countries.geojson)
- [1:10m region label points](https://github.com/nvkelso/natural-earth-vector/blob/ca96624a56bd078437bca8184e78163e5039ad19/geojson/ne_10m_admin_0_countries.geojson)

Natural Earth data is [public domain](https://www.naturalearthdata.com/about/terms-of-use/).
The outline is intentionally low resolution. Label points represent countries
and regions, not server coordinates. Small regions such as HK and SG still have
label points even when absent from the low resolution outline.

Regenerate from the repository root with `node scripts/generate-topology-map.mjs`.
This development command downloads the pinned source data and uses the existing
D3 dependency to project it to a 960 × 460 SVG canvas. The generated data is bundled;
the application performs no map, geolocation, or tile network requests.

Only core-reported source/destination GeoIP codes locate connections. Missing,
ambiguous, or unsupported codes remain unknown. Proxy names and flags are never
used to infer a proxy exit location. Route lines connect known endpoint regions,
not measured physical hops or geolocated intermediate proxies.
