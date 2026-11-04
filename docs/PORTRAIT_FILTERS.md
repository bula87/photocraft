# Portrait retouch packages

This fork adds **Filter › Portrait retouch** and nothing else from the retouch project. PhotoCraft still does not ship trained weights. A package is a folder a user (or the owner) can replace without rebuilding PhotoCraft.

Stock PhotoCraft can keep using `pc-retouch apply` from [photocraft-retouch](https://github.com/bula87/photocraft-retouch). That CLI is optional here. This menu is the fork's host for the same packages.

The package contract below is the one this host loads. It matches the layout the retouch repo documents for `retouch package` (`dist/<task>` with `manifest.json` and `model.onnx`). [bula87/photocraft-retouch](https://github.com/bula87/photocraft-retouch) (Origin `tom-allen/photocraft-retouch`) was not readable while this host was written (GitHub 404, Origin 403), so the JSON field names are specified here for `retouch package` to emit. Extra JSON keys are ignored. The Rust scan, menu, blend and ONNX entry points are `crates/portrait-filters`.

## Install a package

Build packages in photocraft-retouch:

```sh
retouch package
```

Copy each `dist/<id>` directory into the Portrait Filters folder. `<id>` is whatever the package manifest says (`heal`, `dodge_burn`, `skin_tone`, or any new id). Each directory needs `manifest.json` and a model file (`model.onnx` unless the manifest names another file in that folder). A new id does not need a PhotoCraft rebuild.

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

**Filter › Portrait retouch** lists one row per package the scan found, then **All**.

| Row | How it runs |
|---|---|
| Each package's `label` (or `name`) | `filter.portraitRetouch.apply` with `{"id":"<package id>"}`. The menu id is `filter.portraitRetouch.pkg.<id>`. |
| All | `filter.portraitRetouch.all` |

There is no Rust enum of filter kinds. Dropping a new folder on disk is enough; the next time the menu opens, the row is there. A package that is not on disk is absent, not a greyed row. A build without ONNX Runtime still lists the row and disables it, and says to rebuild with `--features portrait-onnx`.

`filter.portraitRetouch.heal`, `filter.portraitRetouch.dodgeBurn` and `filter.portraitRetouch.skinTone` stay as command aliases for the classic ids. They are not separate menu rows.

`filter.portraitRetouch.packages` lists the scan (not a menu item), in apply order.

Untrusted automation (MCP without a filesystem capability) cannot run these commands: they read a user directory. The desktop menu and a trusted local CLI can.

## What a run does

The command reads the active RGB pixel layer (8, 16 or 32-bit), runs that package's graph, and adds a **new layer**. The original layer is not painted. One undo removes every layer that command added.

| Package | Alone | When All places more than one layer | Layer pixels |
|---|---|---|---|
| `heal` | Normal, opacity 1 | Normal, opacity 1 | model RGB |
| `dodge_burn` | Soft Light, opacity 0.85 | Soft Light, opacity 0.85 | model output (RGB, or one gray channel copied to R=G=B). The graph is already the Soft Light map; 0.5 stays neutral. |
| `skin_tone` | Linear Light, opacity 1 | Linear Light, opacity 1 | mid-gray difference |
| any other id | Normal, opacity 1 | same as alone, unless the manifest sets `whenStacked` | model RGB, unless `result` is `difference` |

**All** runs every discovered package, bottom to top. Order is ascending `priority`, then id. The classic ids default to priority 10 (`heal`), 20 (`dodge_burn`) and 30 (`skin_tone`). Any other id defaults to 100, so it sits above those three unless its manifest sets `priority`. One package on its own uses the "alone" blend, including when it is the only package All runs.

### Difference maps

Skin Tone writes a mid-gray residual. Viewed on its own, the layer is flat gray where the model left the photo unchanged, and the edit shows up where it did not. For each channel, `source` is the active-layer RGB fed to the model and `filtered` is the model RGB:

```text
layer = clamp(0.5 + 0.5 * (filtered - source), 0, 1)
```

Linear Light at opacity 1 over that source reconstructs the filtered image:

```text
composite = clamp(source + 2 * layer - 1, 0, 1)
```

Samples already in 0–1 reconstruct exactly. The clamp only moves a sample that was outside 0–1, including infinity. A NaN sample is stored as 0.5, which Linear Light leaves unchanged. Skin Tone uses this residual and Linear Light both alone and when All stacks it. Any other package opts in with `"result": "difference"`. `"result": "absolute"`, or leaving the field out, stores the model RGB. An old Skin Tone manifest that omits `result` still gets the difference. `blend` is independent: a manifest that sets `blend` keeps that mode, and Skin Tone with `"result": "absolute"` stores the model RGB under Linear Light unless it also sets `blend`.

Grayscale, CMYK and Lab documents are refused. Images over 48 megapixels are refused. The model sees finite RGB samples in 0–1. The new layer stores either that RGB or the difference residual, and source alpha is copied onto it.

## manifest.json

`formatVersion` is 1. `retouch package` should write one directory per filter, `dist/<id>/manifest.json` next to the model file. A new id is a new package; this fork does not grow a match arm for it.

```json
{
  "format": "photocraft-portrait-filter",
  "formatVersion": 1,
  "id": "glow",
  "label": "Glow",
  "description": "Optional sentence for the package list.",
  "version": "1.0.0",
  "model": "model.onnx",
  "blend": "screen",
  "opacity": 0.6,
  "layerName": "Glow",
  "priority": 40,
  "whenStacked": { "blend": "screen", "opacity": 0.6 },
  "tile": { "size": 512, "overlap": 32 },
  "tensor": {
    "layout": "nchw",
    "input": "image",
    "output": "image_out"
  }
}
```

| Field | Required | Meaning |
|---|---|---|
| `formatVersion` | when present, must be 1 | Schema version. Absent is version 1. |
| `id` or `task` | yes, or the folder name | Stable package id, 1–64 characters: letters, digits, `.`, `_`, `-`. `heal`, `dodge_burn` / `dodge-burn`, and `skin_tone` / `skin-tone` fold to those three ids. |
| `label` or `name` | no | Menu label. Default is Heal, Dodge & Burn, Skin Tone, or the id. |
| `layerName` | no | New layer's name. Default is the label. |
| `description` | no | Shown by `filter.portraitRetouch.packages`. |
| `blend`, `opacity` | no | Layer blend and opacity, 0–1. Defaults are the table above. `placement.blend` / `placement.opacity` still work. |
| `whenStacked` | no | Blend and opacity when All places more than one layer. |
| `result`, `resultMode`, or `result_mode` | no | `absolute` (alias `full`) stores the model RGB. `difference` (aliases `diff`, `residual`) stores the mid-gray residual from [Difference maps](#difference-maps). Skin Tone defaults to `difference` when the field is absent. Every other id defaults to `absolute`. |
| `priority` or `order` | no | Whole number from -10000 to 10000. Lower is underneath. |
| `model` | no | File name inside the package. Default `model.onnx`. Not a path. |
| `tile.size`, `tile.overlap` | no | Owned square (1–4096) and extra context cropped off the result. Omit to run the whole photo in one pass. |
| `tensor` | no | `layout` `nchw` (default) or `nhwc`, plus optional input and output names. |

`format` may also be `photocraft-retouch-filter` or `photocraft-retouch`. Any other key is ignored.

`tensor.layout` `nchw` is `[1, 3, H, W]`; `nhwc` is `[1, H, W, 3]`. A one-channel gray map is accepted too: NCHW `[1, 1, H, W]` or NHWC `[1, H, W, 1]`. Each gray sample is copied to R, G, and B, so `0.5` stays `0.5` on every channel (neutral under Soft Light). Omitted input and output names use the graph's first input and first output. A fixed height and width on the graph are resized to with a bilinear filter, then the result is resized back. A dynamic height and width run at the photo's size, or at each tile when `tile` is set. Samples are float32 in 0–1.

The same manifest with `"task": "dodge_burn"` and no blend or result fields still means Soft Light at 0.85 and the model output. A one-channel gray map from that graph is copied to R=G=B. `"task": "heal"` is Normal at 1 and the model RGB. `"task": "skin_tone"` is a difference layer at Linear Light, opacity 1, alone and when All stacks it.

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

**All** is the only static row under Filter › Portrait retouch. Package rows are inserted from the scan (`crates/ui-egui/src/menus/portrait_ui.rs`) and call `filter.portraitRetouch.apply`. Adding a package does not add a command.

## Tests

`crates/portrait-filters/tests/fixtures/` holds three synthetic graphs (identity, multiply by 0.5, fixed 4×4 identity), regenerated by `tests/fixtures/generate.py`. They are not trained filters. The engine tests place those contract blends as new layers and check that one undo removes them.
