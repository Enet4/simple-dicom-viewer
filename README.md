# My simple DICOM viewer

This is an experimental DICOM Web viewer, written in Rust.

The viewer uses [DICOM-rs] to deliver a viewer proof of concept,
using WebAssembly.

**Note:** This viewer does not intend to be suitable for clinical purposes.

[DICOM-rs]: https://github.com/Enet4/dicom-rs

## How to install

```sh
npm install
```

## How to run in debug mode

```sh
# Builds the project and opens it in a new browser tab. Auto-reloads when the project changes.
npm start
```

## How to build in release mode

```sh
# Builds the project and places it into the `dist` folder.
npm run build
```
