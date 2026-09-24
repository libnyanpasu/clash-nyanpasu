export default {
  profiles: {
    input: {
      target: "./openapi.json",
    },
    output: {
      mode: "tags-split",
      target: "./frontend/generated/profiles.ts",
      schemas: "./frontend/generated/model",
      client: "react-query",
      httpClient: "fetch",
      override: {
        query: {
          version: 5,
          useInvalidate: true,
          usePrefetch: true,
        },
        mutator: {
          path: "./frontend/api-transport.ts",
          name: "apiFetch",
        },
      },
    },
  },
};
