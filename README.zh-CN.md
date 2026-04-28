# imgsheet

`imgsheet` 是一个把多张图片合成一张总览图的命令行工具，适合给其他程序调用。

## 特性

- 将一个图片目录合成为一张总览图
- 支持 WebP、JPEG、PNG 输出
- 每次运行都会创建新的输出目录，不覆盖旧结果
- 支持 JSON 输出，方便程序解析
- 输入目录必须只包含图片，坏图片会直接报错

## 构建

```bash
cargo build --release
```

生成的可执行文件：

```bash
./target/release/imgsheet
```

也可以在 GitHub Release 下载对应系统的程序：

- Linux x64
- macOS Apple Silicon
- Windows x64

## 基本用法

```bash
imgsheet <输入图片目录> <输出根目录> [参数]
```

示例：

```bash
./target/release/imgsheet \
  /Users/bran/project/ppt/testdata/pages \
  /tmp/imgsheet-runs \
  --json
```

每次运行都会在输出根目录下创建一个新的 `sheet-*` 子目录：

```text
/tmp/imgsheet-runs/sheet-1777362742043867000-18532-0/overview.webp
```

## JSON 调用示例

```bash
./target/release/imgsheet ./slides ./out --format webp --json
```

成功返回：

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

失败返回：

```json
{
  "success": false,
  "message": "invalid image file ./slides/bad.png: Format error decoding Png: Invalid PNG signature."
}
```

## 参数

```text
--format <png|jpeg|webp>        输出格式，默认 webp
--quality <1-100>               JPEG/WebP 质量，默认 80
--canvas-width <px>             画布宽度，默认 1600
--margin <px>                   外边距，默认 64
--gap <px>                      图片间距，默认 32
--card-padding <px>             图片内边距，默认 0
--radius <px>                   图片圆角，默认 28
--columns <1-32>                网格列数，默认 2
--layout <hero-grid|grid>       布局，默认 hero-grid
--sort <natural|name>           排序方式，默认 natural
--recursive                     递归扫描输入目录
--background <#rrggbb>          背景色，默认 #fafaf8
--json                          输出 JSON
```

## 输入规则

支持的输入图片：

- PNG
- JPEG
- WebP

规则：

- 输入目录和输出目录不能相同
- 输入目录下必须全部是支持的图片
- 非递归模式下，输入目录中不能包含子目录
- 递归模式下，所有子目录里的文件也必须是支持的图片
- 坏图片会报错，并指出具体文件

## 输出规则

第二个参数是输出根目录，不是输出文件路径。

每次运行都会创建新的子目录：

```text
<输出根目录>/sheet-<timestamp>-<pid>-<n>/
```

输出文件名固定为：

- `overview.webp`
- `overview.jpeg`
- `overview.png`

失败时会清理本次创建的输出目录。
