const path = require("path");
const { getDefaultConfig } = require("expo/metro-config");

const config = getDefaultConfig(__dirname);
// Faction names and the theme rule live in the web app. Metro otherwise refuses files outside this folder.
config.watchFolders = [path.resolve(__dirname, "../web/lib")];

module.exports = config;
