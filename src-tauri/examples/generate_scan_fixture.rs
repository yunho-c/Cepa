use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Instant;

const DEFAULT_DIRECTORIES: usize = 100;
const DEFAULT_FILES_PER_DIRECTORY: usize = 100;
const DEFAULT_LOGICAL_BYTES_PER_FILE: u64 = 0;
const DIRECTORIES_PER_GROUP: usize = 100;
const MANIFEST_NAME: &str = ".cepa-benchmark-fixture.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema_version: u32,
    directories: usize,
    files_per_directory: usize,
    logical_bytes_per_file: u64,
    generated_files_including_manifest: usize,
    generated_directories: usize,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fixture generation failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (root, directories, files_per_directory, logical_bytes_per_file) = parse_args()?;
    if root.exists() {
        return Err(format!(
            "{} already exists; choose a new path so no data can be overwritten",
            root.display()
        ));
    }

    let started_at = Instant::now();
    fs::create_dir_all(&root)
        .map_err(|error| format!("could not create {}: {error}", root.display()))?;

    for directory_index in 0..directories {
        let group = root.join(format!(
            "group-{:04}",
            directory_index / DIRECTORIES_PER_GROUP
        ));
        fs::create_dir_all(&group)
            .map_err(|error| format!("could not create {}: {error}", group.display()))?;
        let directory = group.join(format!("directory-{directory_index:06}"));
        fs::create_dir(&directory)
            .map_err(|error| format!("could not create {}: {error}", directory.display()))?;

        for file_index in 0..files_per_directory {
            let file_path = directory.join(format!("file-{file_index:06}.bin"));
            let file = File::create(&file_path)
                .map_err(|error| format!("could not create {}: {error}", file_path.display()))?;
            if logical_bytes_per_file > 0 {
                file.set_len(logical_bytes_per_file)
                    .map_err(|error| format!("could not size {}: {error}", file_path.display()))?;
            }
        }
    }

    let group_count = directories.div_ceil(DIRECTORIES_PER_GROUP);
    let manifest = FixtureManifest {
        schema_version: 1,
        directories,
        files_per_directory,
        logical_bytes_per_file,
        generated_files_including_manifest: directories
            .saturating_mul(files_per_directory)
            .saturating_add(1),
        generated_directories: directories.saturating_add(group_count),
    };
    let manifest_path = root.join(MANIFEST_NAME);
    let manifest_file = File::create(&manifest_path)
        .map_err(|error| format!("could not create {}: {error}", manifest_path.display()))?;
    serde_json::to_writer_pretty(manifest_file, &manifest)
        .map_err(|error| format!("could not write {}: {error}", manifest_path.display()))?;

    println!(
        "generated {} files and {} directories at {} in {:.2} seconds",
        manifest.generated_files_including_manifest,
        manifest.generated_directories,
        root.display(),
        started_at.elapsed().as_secs_f64()
    );
    Ok(())
}

fn parse_args() -> Result<(PathBuf, usize, usize, u64), String> {
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let directories = args
        .next()
        .map(|value| parse_positive_usize(value, "directories"))
        .transpose()?
        .unwrap_or(DEFAULT_DIRECTORIES);
    let files_per_directory = args
        .next()
        .map(|value| parse_positive_usize(value, "files per directory"))
        .transpose()?
        .unwrap_or(DEFAULT_FILES_PER_DIRECTORY);
    let logical_bytes_per_file = args
        .next()
        .map(|value| parse_u64(value, "logical bytes per file"))
        .transpose()?
        .unwrap_or(DEFAULT_LOGICAL_BYTES_PER_FILE);

    if args.next().is_some() {
        return Err(usage());
    }
    validate_parent(&path)?;

    Ok((
        path,
        directories,
        files_per_directory,
        logical_bytes_per_file,
    ))
}

fn validate_parent(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("fixture path cannot be empty".to_string());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        return Err(format!(
            "parent directory {} does not exist",
            parent.display()
        ));
    }
    Ok(())
}

fn parse_positive_usize(value: OsString, name: &str) -> Result<usize, String> {
    let parsed = parse_u64(value, name)?;
    let parsed = usize::try_from(parsed).map_err(|_| format!("{name} is too large"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn parse_u64(value: OsString, name: &str) -> Result<u64, String> {
    value
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))?
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a non-negative integer"))
}

fn usage() -> String {
    "usage: generate_scan_fixture <new-path> [directories] [files-per-directory] [logical-bytes-per-file]".to_string()
}
