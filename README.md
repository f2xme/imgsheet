# imgsheet

`imgsheet` is a small CLI that combines a directory of slide images into one overview image.

[中文文档](README.zh-CN.md)

It is designed for programmatic use:

- input must be a directory containing only supported image files
- `INPUT_DIR` and `OUTPUT_DIR` must be different
- every run creates a unique child directory under `OUTPUT_DIR`
- the generated file path is returned in JSON when `--json` is used

## Build

```bash
cargo build --release
```

The binary will be:

```bash
./target/release/imgsheet
```

## Usage

```bash
imgsheet <INPUT_DIR> <OUTPUT_DIR> [OPTIONS]
```

Example:

```bash
./target/release/imgsheet \
  /Users/bran/project/ppt/testdata/pages \
  /tmp/imgsheet-runs \
  --json
```

Each run creates a new directory:

```text
/tmp/imgsheet-runs/sheet-1777362742043867000-18532-0/overview.webp
```

## JSON Example

```bash
./target/release/imgsheet ./slides ./out --format webp --json
```

Success:

```json
{
  "success": true,
  "image_count": 23,
  "canvas_width": 1600,
  "canvas_height": 5763,
  "output_dir": "./out/sheet-1777362742043867000-18532-0",
  "output_file": "./out/sheet-1777362742043867000-18532-0/overview.webp",
  "output_format": "webp",
  "layout": "hero-grid",
  "sort": "natural",
  "recursive": false,
  "source_files": ["./slides/slide-01.png"]
}
```

Failure:

```json
{
  "success": false,
  "message": "invalid image file ./slides/bad.png: Format error decoding Png: Invalid PNG signature."
}
```

## Options

```text
--format <png|jpeg|webp>        Output format, default webp
--quality <1-100>               JPEG/WebP quality, default 80
--canvas-width <px>             Canvas width, default 1600
--margin <px>                   Outer margin, default 64
--gap <px>                      Gap between images, default 32
--card-padding <px>             Padding inside each image tile, default 0
--radius <px>                   Rounded image corner radius, default 28
--columns <1-32>                Grid columns, default 2
--layout <hero-grid|grid>       Layout mode, default hero-grid
--sort <natural|name>           Sort mode, default natural
--recursive                     Recursively scan input directories
--background <#rrggbb>          Canvas background color, default #fafaf8
--json                          Print machine-readable JSON
```

## Input Rules

Supported input files:

- PNG
- JPEG
- WebP

The input directory must contain only supported images. In non-recursive mode, subdirectories are rejected. In recursive mode, every nested file must also be a supported image.

Bad image files are reported as errors and the run output directory is cleaned up.

## Output Rules

`OUTPUT_DIR` is a root directory. `imgsheet` creates a new `sheet-*` directory under it on every run.

Generated file names:

- `overview.webp`
- `overview.jpeg`
- `overview.png`
