# Portrait retouch packages

This fork adds **Filter › Portrait retouch** and nothing else from the retouch project. PhotoCraft still does not ship trained weights. A package is a folder a user (or the owner) can replace without rebuilding PhotoCraft.

Stock PhotoCraft can keep using `pc-retouch apply` from [photocraft-retouch](https://github.com/bula87/photocraft-retouch). That CLI is optional here. This menu is the fork's host for the same packages.

The package contract below is the one this host loads. It matches the layout the retouch repo documents for `retouch package` (`dist/<task>` with `manifest.json` and `model.onnx`). [bula87/photocraft-retouch](https://github.com/bula87/photocraft-retouch) (Origin `tom-allen/photocraft-retouch`) was not readable while this host was written (GitHub 404, Origin 403), so the JSON field names are specified here for `retouch package` to emit. Extra JSON keys are ignored. The Rust scan, menu, blend and ONNX entry points are `crates/portrait-filters`.

## Install a package

Build packages in photocraft-retouch:

```sh
retouch package
```

Copy each `dist/<task>` directory into the Portrait Filters folder. `<task>` is `heal`, `dodge_burn` or `skin_tone`. Each directory needs `manifest.json` and `model.onnx`.

| Platform | Default folder |
|---|---|
| macOS | `~/Library/Application Support/PhotoCraft/Portrait Filters` |
| Linux, Windows, and everywhere else | `$XDG_DATA_HOME/photocraft/portrait-filters`, or `~/.local/share/photocraft/portrait-filters` when `XDG_DATA_HOME` is unset |

Override the folder in **Edit › Preferences › Plug-ins › Portrait Filters Folder**. An empty value keeps the default. The web build has no portrait-filter runtime.

Example:

```text
~/.local/share/photocraft/portrait-filters/
  heal/
    manifest.json
    model.onnx
  dodge_burn/
    manifest.json
    model.onnx
  skin_tone/
    manifest.json
    model.onnx
```

Restart is not required. The menu scans the folder when it opens.

## Menu

**Filter › Portrait retouch**

| Item | Command | Needs |
|---|---|---|
| Heal | `filter.portraitRetouch.heal` | the `heal` package |
| Dodge & Burn | `filter.portraitRetouch.dodgeBurn` | the `dodge_burn` package |
| Skin Tone | `filter.portraitRetouch.skinTone` | the `skin_tone` package |
| All | `filter.portraitRetouch.all` | all three packages |

A missing package is disabled, with the folder in the reason. **All** is disabled until every package is present. A build without ONNX Runtime disables an item that does have its package, and says to rebuild with `--features portrait-onnx`.

`filter.portraitRetouch.packages` lists what the scan found (not a menu item).

Untrusted automation (MCP without a filesystem capability) cannot run these commands: they read a user directory. The desktop menu and a trusted local CLI can.

## What a run does

The command reads the active RGB pixel layer (8, 16 or 32-bit), runs that package's graph, and adds a **new layer**. The original layer is not painted. One undo removes every layer that command added.

| Task | Alone | In the All stack |
|---|---|---|
| Heal | Normal, opacity 1 | Normal, opacity 1 |
| Dodge & Burn | Soft Light, opacity 0.85 | Soft Light, opacity 0.85 |
| Skin Tone | Normal, opacity 1 | Color, opacity 1 |

All stacks them bottom to top: Heal, Dodge & Burn, Skin Tone. Skin Tone uses Color only in that stack, so it recolours the layers under it and leaves their luminosity alone. A lone Skin Tone layer keeps the model's RGB (Normal).

Grayscale, CMYK and Lab documents are refused. Images over 48 megapixels are refused. The model sees finite RGB samples in 0–1. Source alpha is copied onto the new layer.

## manifest.json

```json
{
  "format": "photocraft-portrait-filter",
  "formatVersion": 1,
  "task": "dodge_burn",
  "name": "Dodge & Burn",
  "version": "1.0.0",
  "model": "model.onnx",
  "placement": {
    "blend": "soft light",
    "opacity": 0.85,
    "whenStacked": { "blend": "soft light", "opacity": 0.85 }
  },
  "tensor": {
    "layout": "nchw",
    "input": "image",
    "output": "image_out"
  }
}
```

`format` may also be `photocraft-retouch-filter` or `photocraft-retouch`. `formatVersion` must be 1 when present. `task` may be omitted when the folder name is `heal`, `dodge_burn` or `skin_tone` (`dodge-burn` and `skin-tone` are accepted). `model` defaults to `model.onnx` and must be a file name inside the package, not a path.

`placement` is optional. Omitted fields use the table above. `tensor.layout` is `nchw` (`[1, 3, H, W]`, the default) or `nhwc` (`[1, H, W, 3]`). Omitted input and output names use the graph's first input and first output. A fixed height and width on the graph are resized to with a bilinear filter, then the result is resized back to the photo. A dynamic height and width run at the photo's size. Samples are float32 in 0–1.

`retouch package` should write this manifest next to `model.onnx` under `dist/<task>`.

## Build

PhotoCraft's toolchain is the one in the workspace `Cargo.toml` (`rust-version`, currently 1.95 or newer).

Default build, no ONNX Runtime (what wasm and a normal `cargo build` use):

```sh
cargo test -p photocraft-portrait-filters
cargo build -p photocraft
```

Native build that can run packages:

```sh
cargo build -p photocraft --features portrait-onnx
cargo build -p photocraft-cli --features portrait-onnx
```

The feature downloads ONNX Runtime through the `ort` crate (MIT OR Apache-2.0, ONNX Runtime 1.31). Do not pass `portrait-onnx` to the web package. `cargo check -p photocraft-portrait-filters --target wasm32-unknown-unknown` stays free of that runtime.

Filter menu items are engine commands, so they show up from the same menu builder as every other extra command. No menu-catalog edit is required.

## Tests

`crates/portrait-filters/tests/fixtures/` holds three synthetic graphs (identity, multiply by 0.5, fixed 4×4 identity), regenerated by `tests/fixtures/generate.py`. They are not trained filters. The engine tests place those contract blends as new layers and check that one undo removes them.
