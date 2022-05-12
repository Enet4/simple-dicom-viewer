const path = require("path");
const CopyPlugin = require("copy-webpack-plugin");
const WasmPackPlugin = require("@wasm-tool/wasm-pack-plugin");

const dist = path.resolve(__dirname, "dist");

module.exports = {
  mode: "production",
  entry: {
    index: "./js/index.js"
  },
  output: {
    path: dist,
    filename: "[name].js"
  },
  devServer: {
    static: "./dist",
    client: {
      overlay: {
        errors: true,
        warnings: false,
      },
    }
  },
  watchOptions: {
    aggregateTimeout: 500,
    poll: 250,
  },
  plugins: [
    new CopyPlugin({
      patterns: [
        "static"
      ],
    }),
    new WasmPackPlugin({
      crateDirectory: __dirname,
      //forceMode: "development",
    }),
  ],
  experiments: {
    syncWebAssembly: true,
  }
};
