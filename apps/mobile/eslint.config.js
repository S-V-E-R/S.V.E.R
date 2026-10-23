const expoConfig = require("eslint-config-expo/flat");
const { defineConfig } = require("eslint/config");

module.exports = defineConfig([
  expoConfig,
  {
    ignores: ["dist/**", ".expo/**", "node_modules/**", "android/**", "ios/**"],
  },
  {
    rules: {
      // The web faction module is the source of those names; this app only reads it.
      "import/no-relative-packages": "off",
    },
  },
]);
