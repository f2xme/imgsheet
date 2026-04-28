use clap::{Parser, ValueEnum};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{
    ColorType, DynamicImage, GenericImageView, ImageBuffer, ImageEncoder, Rgba, RgbaImage,
};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};
use webpx::{Encoder as WebpEncoder, Unstoppable};

type AppResult<T> = Result<T, String>;

#[derive(Debug, Parser)]
#[command(
    name = "imgsheet",
    about = "Compose slide images into one overview image."
)]
struct Cli {
    #[arg(help = "Directory containing slide images")]
    input_dir: PathBuf,

    #[arg(help = "Output root directory. A unique child directory is created on every run.")]
    output_dir: PathBuf,

    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Webp,
        help = "Output format."
    )]
    format: OutputFormat,

    #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,

    #[arg(long = "canvas-width", default_value_t = 1600)]
    canvas_width: u32,

    #[arg(long, default_value_t = 64)]
    margin: u32,

    #[arg(long, default_value_t = 32)]
    gap: u32,

    #[arg(long = "card-padding", default_value_t = 0)]
    card_padding: u32,

    #[arg(long, default_value_t = 28)]
    radius: u32,

    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=32))]
    columns: u32,

    #[arg(long, value_enum, default_value_t = LayoutMode::HeroGrid)]
    layout: LayoutMode,

    #[arg(long, value_enum, default_value_t = SortMode::Natural)]
    sort: SortMode,

    #[arg(long, help = "Scan input directory recursively")]
    recursive: bool,

    #[arg(long, default_value = "#fafaf8", help = "Canvas background color")]
    background: String,

    #[arg(long, help = "Print machine-readable JSON result")]
    json: bool,
}

#[derive(Debug)]
struct Config {
    input_dir: PathBuf,
    output_dir: PathBuf,
    output_format: OutputFormat,
    quality: u8,
    canvas_width: u32,
    margin: u32,
    gap: u32,
    card_padding: u32,
    radius: u32,
    columns: u32,
    layout: LayoutMode,
    sort: SortMode,
    recursive: bool,
    background: Rgba<u8>,
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Png,
    Jpeg,
    Webp,
}

impl OutputFormat {
    fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpeg",
            OutputFormat::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LayoutMode {
    HeroGrid,
    Grid,
}

impl LayoutMode {
    fn as_str(self) -> &'static str {
        match self {
            LayoutMode::HeroGrid => "hero-grid",
            LayoutMode::Grid => "grid",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SortMode {
    Name,
    Natural,
}

impl SortMode {
    fn as_str(self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::Natural => "natural",
        }
    }
}

#[derive(Debug)]
struct LoadedImage {
    path: PathBuf,
    image: DynamicImage,
}

#[derive(Debug)]
struct GridLayout {
    image_index: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct SheetSummary {
    image_count: usize,
    canvas_width: u32,
    canvas_height: u32,
    output_dir: PathBuf,
    output_file: PathBuf,
    output_format: OutputFormat,
    layout: LayoutMode,
    sort: SortMode,
    recursive: bool,
    source_files: Vec<PathBuf>,
}

fn main() {
    let json_requested = std::env::args_os().any(|arg| arg == "--json");

    match Cli::try_parse() {
        Ok(cli) => {
            if let Err(err) = run(cli) {
                print_error(&err, json_requested);
                process::exit(1);
            }
        }
        Err(err) => {
            if err.exit_code() == 0 {
                let _ = err.print();
                process::exit(0);
            }
            if json_requested {
                print_error(&err.to_string(), true);
                process::exit(err.exit_code());
            }
            let _ = err.print();
            process::exit(err.exit_code());
        }
    }
}

fn run(cli: Cli) -> AppResult<()> {
    let config = Config::try_from(cli)?;
    validate_config(&config)?;

    let output_dir = create_run_output_dir(&config.output_dir)?;
    let run_result = run_with_output_dir(&config, output_dir.clone());
    if run_result.is_err() {
        cleanup_output_dir(&output_dir);
    }
    run_result
}

fn run_with_output_dir(config: &Config, output_dir: PathBuf) -> AppResult<()> {
    let images = load_images(config)?;
    if images.is_empty() {
        return Err(format!(
            "no supported images found in {:?}",
            config.input_dir
        ));
    }

    let sheet = build_sheet(&images, config)?;
    let output_file = output_dir.join(format!("overview.{}", config.output_format.as_str()));
    write_image(&sheet, &output_file, config.output_format, config.quality)?;

    let summary = SheetSummary {
        image_count: images.len(),
        canvas_width: sheet.width(),
        canvas_height: sheet.height(),
        output_dir,
        output_file,
        output_format: config.output_format,
        layout: config.layout,
        sort: config.sort,
        recursive: config.recursive,
        source_files: images.iter().map(|loaded| loaded.path.clone()).collect(),
    };

    if config.json {
        println!("{}", render_json_summary(&summary));
    } else {
        println!(
            "Saved {}x{} {} sheet with {} image(s) to {}",
            summary.canvas_width,
            summary.canvas_height,
            summary.output_format.as_str(),
            summary.image_count,
            summary.output_file.display()
        );
    }

    Ok(())
}

fn print_error(message: &str, json: bool) {
    if json {
        println!("{}", render_json_error(message));
    } else {
        eprintln!("Error: {message}");
    }
}

impl TryFrom<Cli> for Config {
    type Error = String;

    fn try_from(value: Cli) -> AppResult<Self> {
        Ok(Self {
            input_dir: value.input_dir,
            output_dir: value.output_dir,
            output_format: value.format,
            quality: value.quality,
            canvas_width: value.canvas_width,
            margin: value.margin,
            gap: value.gap,
            card_padding: value.card_padding,
            radius: value.radius,
            columns: value.columns,
            layout: value.layout,
            sort: value.sort,
            recursive: value.recursive,
            background: parse_hex_color(&value.background)?,
            json: value.json,
        })
    }
}

fn validate_config(config: &Config) -> AppResult<()> {
    if !config.input_dir.is_dir() {
        return Err(format!(
            "input dir does not exist or is not a directory: {}",
            config.input_dir.display()
        ));
    }

    if config.output_dir.exists() && !config.output_dir.is_dir() {
        return Err(format!(
            "output dir exists but is not a directory: {}",
            config.output_dir.display()
        ));
    }

    if same_path(&config.input_dir, &config.output_dir) {
        return Err("input dir and output dir must be different".to_string());
    }

    validate_input_images_only(&config.input_dir, config.recursive, &config.output_dir)?;

    let double_margin = config
        .margin
        .checked_mul(2)
        .ok_or_else(|| "margin is too large".to_string())?;
    if double_margin >= config.canvas_width {
        return Err("canvas width must be larger than margin * 2".to_string());
    }

    let content_width = config.canvas_width - double_margin;
    let total_gap = config
        .gap
        .checked_mul(config.columns.saturating_sub(1))
        .ok_or_else(|| "gap is too large".to_string())?;
    if total_gap >= content_width {
        return Err("canvas width is too small for the requested columns and gap".to_string());
    }

    let grid_width = (content_width - total_gap) / config.columns;
    let double_padding = config
        .card_padding
        .checked_mul(2)
        .ok_or_else(|| "card padding is too large".to_string())?;
    if double_padding >= grid_width {
        return Err("card padding leaves no drawable image area".to_string());
    }

    Ok(())
}

fn validate_input_images_only(
    input_dir: &Path,
    recursive: bool,
    output_dir: &Path,
) -> AppResult<()> {
    for entry in fs::read_dir(input_dir)
        .map_err(|err| format!("failed to read {}: {err}", input_dir.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            if same_path(&path, output_dir) {
                continue;
            }
            if !recursive {
                return Err(format!(
                    "input dir contains a subdirectory but --recursive is not enabled: {}",
                    path.display()
                ));
            }
            validate_input_images_only(&path, recursive, output_dir)?;
        } else if !path.is_file() || !is_supported_image(&path) {
            return Err(format!(
                "input dir contains a non-image file: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn parse_hex_color(value: &str) -> AppResult<Rgba<u8>> {
    let color = value.strip_prefix('#').unwrap_or(value);
    if !color.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("invalid background color: {value}"));
    }

    let bytes = color.as_bytes();
    let parse_pair = |start: usize| -> AppResult<u8> {
        let high =
            hex_value(bytes[start]).ok_or_else(|| format!("invalid background color: {value}"))?;
        let low = hex_value(bytes[start + 1])
            .ok_or_else(|| format!("invalid background color: {value}"))?;
        Ok(high * 16 + low)
    };

    match color.len() {
        6 => Ok(Rgba([parse_pair(0)?, parse_pair(2)?, parse_pair(4)?, 255])),
        8 => Ok(Rgba([
            parse_pair(0)?,
            parse_pair(2)?,
            parse_pair(4)?,
            parse_pair(6)?,
        ])),
        _ => Err(format!(
            "invalid background color: {value} (expected #rrggbb or #rrggbbaa)"
        )),
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn load_images(config: &Config) -> AppResult<Vec<LoadedImage>> {
    let mut paths = discover_image_paths(&config.input_dir, config.recursive, &config.output_dir)?;
    sort_paths(&mut paths, config.sort);

    let mut images = Vec::with_capacity(paths.len());
    for path in paths {
        let image = image::open(&path)
            .map_err(|err| format!("invalid image file {}: {err}", path.display()))?;
        images.push(LoadedImage { path, image });
    }

    Ok(images)
}

fn discover_image_paths(
    input_dir: &Path,
    recursive: bool,
    output_dir: &Path,
) -> AppResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_image_paths(input_dir, recursive, output_dir, &mut paths)?;
    Ok(paths)
}

fn collect_image_paths(
    input_dir: &Path,
    recursive: bool,
    output_dir: &Path,
    paths: &mut Vec<PathBuf>,
) -> AppResult<()> {
    for entry in fs::read_dir(input_dir)
        .map_err(|err| format!("failed to read {}: {err}", input_dir.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                if same_path(&path, output_dir) {
                    continue;
                }
                collect_image_paths(&path, recursive, output_dir, paths)?;
            }
        } else if path.is_file() && is_supported_image(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

fn sort_paths(paths: &mut [PathBuf], sort: SortMode) {
    match sort {
        SortMode::Name => paths.sort(),
        SortMode::Natural => paths.sort_by(|left, right| natural_path_cmp(left, right)),
    }
}

fn natural_path_cmp(left: &Path, right: &Path) -> Ordering {
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();
    natural_str_cmp(&left, &right)
}

fn natural_str_cmp(left: &str, right: &str) -> Ordering {
    let mut left_iter = left.char_indices().peekable();
    let mut right_iter = right.char_indices().peekable();

    while left_iter.peek().is_some() && right_iter.peek().is_some() {
        let (_, left_ch) = *left_iter.peek().expect("peeked above");
        let (_, right_ch) = *right_iter.peek().expect("peeked above");

        if left_ch.is_ascii_digit() && right_ch.is_ascii_digit() {
            let left_number = take_number(left, &mut left_iter);
            let right_number = take_number(right, &mut right_iter);
            let ordering = compare_number_chunks(left_number, right_number);
            if ordering != Ordering::Equal {
                return ordering;
            }
        } else {
            left_iter.next();
            right_iter.next();
            let ordering = left_ch
                .to_ascii_lowercase()
                .cmp(&right_ch.to_ascii_lowercase());
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
    }

    left.len().cmp(&right.len())
}

fn take_number<'a>(
    source: &'a str,
    iter: &mut std::iter::Peekable<std::str::CharIndices<'a>>,
) -> &'a str {
    let start = iter.peek().map(|(idx, _)| *idx).unwrap_or(source.len());
    let mut end = start;
    while let Some((idx, ch)) = iter.peek().copied() {
        if !ch.is_ascii_digit() {
            break;
        }
        end = idx + ch.len_utf8();
        iter.next();
    }
    &source[start..end]
}

fn compare_number_chunks(left: &str, right: &str) -> Ordering {
    let left_trimmed = left.trim_start_matches('0');
    let right_trimmed = right.trim_start_matches('0');
    let left_number = if left_trimmed.is_empty() {
        "0"
    } else {
        left_trimmed
    };
    let right_number = if right_trimmed.is_empty() {
        "0"
    } else {
        right_trimmed
    };

    left_number
        .len()
        .cmp(&right_number.len())
        .then_with(|| left_number.cmp(right_number))
        .then_with(|| left.len().cmp(&right.len()))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
        .unwrap_or(false)
}

fn build_sheet(images: &[LoadedImage], config: &Config) -> AppResult<DynamicImage> {
    let content_width = config.canvas_width - config.margin * 2;
    let grid_width = if config.columns > 1 {
        (content_width - config.gap * (config.columns - 1)) / config.columns
    } else {
        content_width
    };

    match config.layout {
        LayoutMode::HeroGrid => build_hero_grid_sheet(images, config, content_width, grid_width),
        LayoutMode::Grid => build_grid_sheet(images, config, content_width, grid_width),
    }
}

fn build_hero_grid_sheet(
    images: &[LoadedImage],
    config: &Config,
    content_width: u32,
    grid_width: u32,
) -> AppResult<DynamicImage> {
    let first = images
        .first()
        .ok_or_else(|| "no images provided to build_sheet".to_string())?;
    let hero_height =
        tile_height_for_image(first.image.dimensions(), content_width, config.card_padding);

    let grid_layouts = build_grid_layouts(images, 1, config, content_width, grid_width);
    let grid_total_height = total_layout_height(&grid_layouts);

    let content_height = if grid_total_height == 0 {
        hero_height
    } else {
        hero_height + config.gap + grid_total_height
    };
    let canvas_height = config.margin * 2 + content_height;

    let mut canvas = ImageBuffer::from_pixel(config.canvas_width, canvas_height, config.background);

    draw_card(
        &mut canvas,
        Rect {
            x: config.margin,
            y: config.margin,
            width: content_width,
            height: hero_height,
        },
        config.radius,
        &first.image,
        config.card_padding,
    );

    for layout in &grid_layouts {
        draw_card(
            &mut canvas,
            Rect {
                x: config.margin + layout.x,
                y: config.margin + hero_height + config.gap + layout.y,
                width: layout.width,
                height: layout.height,
            },
            config.radius,
            &images[layout.image_index].image,
            config.card_padding,
        );
    }

    Ok(DynamicImage::ImageRgba8(canvas))
}

fn build_grid_sheet(
    images: &[LoadedImage],
    config: &Config,
    content_width: u32,
    grid_width: u32,
) -> AppResult<DynamicImage> {
    let grid_layouts = build_grid_layouts(images, 0, config, content_width, grid_width);
    let canvas_height = config.margin * 2 + total_layout_height(&grid_layouts);
    let mut canvas = ImageBuffer::from_pixel(config.canvas_width, canvas_height, config.background);

    for layout in &grid_layouts {
        draw_card(
            &mut canvas,
            Rect {
                x: config.margin + layout.x,
                y: config.margin + layout.y,
                width: layout.width,
                height: layout.height,
            },
            config.radius,
            &images[layout.image_index].image,
            config.card_padding,
        );
    }

    Ok(DynamicImage::ImageRgba8(canvas))
}

fn total_layout_height(layouts: &[GridLayout]) -> u32 {
    layouts
        .iter()
        .map(|layout| layout.y + layout.height)
        .max()
        .unwrap_or(0)
}

fn build_grid_layouts(
    images: &[LoadedImage],
    start_index: usize,
    config: &Config,
    full_width: u32,
    grid_width: u32,
) -> Vec<GridLayout> {
    let mut layouts = Vec::new();
    if images.len() <= start_index {
        return layouts;
    }

    let mut next_image_index = start_index;
    let mut y = 0_u32;

    while next_image_index < images.len() {
        let remaining = images.len() - next_image_index;

        if remaining == 1 && config.columns > 1 {
            let image_index = next_image_index;
            let height = tile_height_for_image(
                images[image_index].image.dimensions(),
                full_width,
                config.card_padding,
            );
            layouts.push(GridLayout {
                image_index,
                x: 0,
                y,
                width: full_width,
                height,
            });
            break;
        }

        let row_count = remaining.min(config.columns as usize);
        let mut row_height = 0_u32;

        for col in 0..row_count {
            let image_index = next_image_index + col;
            let height = tile_height_for_image(
                images[image_index].image.dimensions(),
                grid_width,
                config.card_padding,
            );
            row_height = row_height.max(height);
            layouts.push(GridLayout {
                image_index,
                x: col as u32 * (grid_width + config.gap),
                y,
                width: grid_width,
                height,
            });
        }

        next_image_index += row_count;
        if next_image_index < images.len() {
            y += row_height + config.gap;
        }
    }

    layouts
}

fn tile_height_for_image(dimensions: (u32, u32), tile_width: u32, padding: u32) -> u32 {
    let (width, height) = dimensions;
    let inner_width = tile_width.saturating_sub(padding * 2);
    if width == 0 || inner_width == 0 {
        return padding * 2;
    }

    let aspect = height as f32 / width as f32;
    let inner_height = ((inner_width as f32) * aspect).round().max(1.0) as u32;
    inner_height + padding * 2
}

fn draw_card(canvas: &mut RgbaImage, rect: Rect, radius: u32, image: &DynamicImage, padding: u32) {
    let inner_x = rect.x + padding;
    let inner_y = rect.y + padding;
    let inner_w = rect.width.saturating_sub(padding * 2);
    let inner_h = rect.height.saturating_sub(padding * 2);

    if inner_w == 0 || inner_h == 0 {
        return;
    }

    let fitted = fit_image(image, inner_w, inner_h);
    let paste_x = inner_x + (inner_w - fitted.width()) / 2;
    let paste_y = inner_y + (inner_h - fitted.height()) / 2;
    overlay_rounded_image(canvas, &fitted.to_rgba8(), paste_x, paste_y, radius);
}

fn fit_image(image: &DynamicImage, max_width: u32, max_height: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width <= max_width && height <= max_height {
        return image.clone();
    }

    let width_ratio = max_width as f32 / width as f32;
    let height_ratio = max_height as f32 / height as f32;
    let ratio = width_ratio.min(height_ratio);

    let target_width = ((width as f32) * ratio).round().max(1.0) as u32;
    let target_height = ((height as f32) * ratio).round().max(1.0) as u32;

    image.resize_exact(target_width, target_height, FilterType::Lanczos3)
}

fn point_in_rounded_rect(
    px: u32,
    py: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }

    let left = x as i64;
    let top = y as i64;
    let right = (x + width - 1) as i64;
    let bottom = (y + height - 1) as i64;
    let px = px as i64;
    let py = py as i64;
    let radius = radius as i64;

    if px >= left + radius && px <= right - radius {
        return py >= top && py <= bottom;
    }

    if py >= top + radius && py <= bottom - radius {
        return px >= left && px <= right;
    }

    let corners = [
        (left + radius, top + radius),
        (right - radius, top + radius),
        (left + radius, bottom - radius),
        (right - radius, bottom - radius),
    ];

    corners.iter().any(|(cx, cy)| {
        let dx = px - *cx;
        let dy = py - *cy;
        dx * dx + dy * dy <= radius * radius
    })
}

fn overlay_rounded_image(canvas: &mut RgbaImage, image: &RgbaImage, x: u32, y: u32, radius: u32) {
    let width = image.width();
    let height = image.height();
    let radius = radius.min(width / 2).min(height / 2);

    for iy in 0..height {
        for ix in 0..width {
            let target_x = x + ix;
            let target_y = y + iy;
            if point_in_rounded_rect(target_x, target_y, x, y, width, height, radius) {
                let pixel = image.get_pixel(ix, iy);
                canvas.put_pixel(target_x, target_y, *pixel);
            }
        }
    }
}

fn write_image(
    image: &DynamicImage,
    output_path: &Path,
    format: OutputFormat,
    quality: u8,
) -> AppResult<()> {
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    let temp_path = temp_output_path(output_path)?;
    let write_result = match format {
        OutputFormat::Png => write_png(image, &temp_path),
        OutputFormat::Jpeg => write_jpeg(image, &temp_path, quality),
        OutputFormat::Webp => write_webp(image, &temp_path, quality),
    };

    if let Err(err) = write_result {
        cleanup_temp_file(&temp_path);
        return Err(err);
    }

    publish_temp_file(&temp_path, output_path)
}

fn create_run_output_dir(output_root: &Path) -> AppResult<PathBuf> {
    fs::create_dir_all(output_root)
        .map_err(|err| format!("failed to create {}: {err}", output_root.display()))?;

    let process_id = process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    for attempt in 0..100_u32 {
        let candidate = output_root.join(format!("sheet-{timestamp}-{process_id}-{attempt}"));
        match fs::create_dir(&candidate) {
            Ok(()) => {
                sync_parent_dir(&candidate)?;
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!("failed to create {}: {err}", candidate.display()));
            }
        }
    }

    Err(format!(
        "failed to allocate unique output directory under {}",
        output_root.display()
    ))
}

fn temp_output_path(output_path: &Path) -> AppResult<PathBuf> {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path
        .file_name()
        .ok_or_else(|| format!("invalid output file name: {}", output_path.display()))?;
    let process_id = process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    for attempt in 0..100_u32 {
        let mut temp_name = OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(".tmp-{process_id}-{timestamp}-{attempt}"));
        let candidate = parent.join(temp_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "failed to allocate temp output path beside {}",
        output_path.display()
    ))
}

fn publish_temp_file(temp_path: &Path, output_path: &Path) -> AppResult<()> {
    fs::rename(temp_path, output_path).map_err(|err| {
        cleanup_temp_file(temp_path);
        format!(
            "failed to move {} to {}: {err}",
            temp_path.display(),
            output_path.display()
        )
    })?;
    sync_parent_dir(output_path)
}

#[cfg(unix)]
fn sync_parent_dir(output_path: &Path) -> AppResult<()> {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|err| {
            format!(
                "failed to sync output directory {}: {err}",
                parent.display()
            )
        })
}

#[cfg(not(unix))]
fn sync_parent_dir(_output_path: &Path) -> AppResult<()> {
    Ok(())
}

fn cleanup_temp_file(path: &Path) {
    if let Err(err) = fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "Warning: failed to remove temp file {}: {err}",
            path.display()
        );
    }
}

fn cleanup_output_dir(path: &Path) {
    if let Err(err) = fs::remove_dir_all(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "Warning: failed to remove output dir {}: {err}",
            path.display()
        );
    }
}

fn write_png(image: &DynamicImage, output_path: &Path) -> AppResult<()> {
    let file = File::create(output_path)
        .map_err(|err| format!("failed to create {}: {err}", output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let rgba = image.to_rgba8();
    PngEncoder::new(&mut writer)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(|err| format!("failed to encode {} as png: {err}", output_path.display()))?;
    finish_writer(output_path, writer)
}

fn write_jpeg(image: &DynamicImage, output_path: &Path, quality: u8) -> AppResult<()> {
    let file = File::create(output_path)
        .map_err(|err| format!("failed to create {}: {err}", output_path.display()))?;
    let mut writer = BufWriter::new(file);
    let mut encoder = JpegEncoder::new_with_quality(&mut writer, quality);
    let rgb = image.to_rgb8();
    encoder
        .encode_image(&rgb)
        .map_err(|err| format!("failed to encode {} as jpeg: {err}", output_path.display()))?;
    drop(encoder);
    finish_writer(output_path, writer)
}

fn write_webp(image: &DynamicImage, output_path: &Path, quality: u8) -> AppResult<()> {
    let rgba = image.to_rgba8();
    let encoded = WebpEncoder::new_rgba(rgba.as_raw(), rgba.width(), rgba.height())
        .quality(quality as f32)
        .encode_owned(Unstoppable)
        .map_err(|err| format!("failed to encode {} as webp: {err}", output_path.display()))?;

    write_all_bytes(output_path, &encoded)
}

fn write_all_bytes(output_path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut file = File::create(output_path)
        .map_err(|err| format!("failed to create {}: {err}", output_path.display()))?;
    file.write_all(bytes)
        .map_err(|err| format!("failed to write {}: {err}", output_path.display()))?;
    file.sync_all()
        .map_err(|err| format!("failed to sync {}: {err}", output_path.display()))
}

fn finish_writer(output_path: &Path, mut writer: BufWriter<File>) -> AppResult<()> {
    writer
        .flush()
        .map_err(|err| format!("failed to flush {}: {err}", output_path.display()))?;
    let file = writer
        .into_inner()
        .map_err(|err| format!("failed to finish {}: {err}", output_path.display()))?;
    file.sync_all()
        .map_err(|err| format!("failed to sync {}: {err}", output_path.display()))
}

fn render_json_summary(summary: &SheetSummary) -> String {
    let sources = summary
        .source_files
        .iter()
        .map(|path| format!("\"{}\"", escape_json(&path.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"success\":true,\"image_count\":{},\"canvas_width\":{},\"canvas_height\":{},\"output_dir\":\"{}\",\"output_file\":\"{}\",\"output_format\":\"{}\",\"layout\":\"{}\",\"sort\":\"{}\",\"recursive\":{},\"source_files\":[{}]}}",
        summary.image_count,
        summary.canvas_width,
        summary.canvas_height,
        escape_json(&summary.output_dir.to_string_lossy()),
        escape_json(&summary.output_file.to_string_lossy()),
        summary.output_format.as_str(),
        summary.layout.as_str(),
        summary.sort.as_str(),
        summary.recursive,
        sources
    )
}

fn render_json_error(message: &str) -> String {
    format!(
        "{{\"success\":false,\"message\":\"{}\"}}",
        escape_json(message)
    )
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}
