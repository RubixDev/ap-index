use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    hash::{DefaultHasher, Hash, Hasher as _},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    process::Command,
    sync::LazyLock,
};

use anyhow::{Context, Result};
use digest_io::IoWrapper;
use reqwest::blocking::Client;
use semver::Version;
use serde_with::{NoneAsEmptyString, hex::Hex, serde_as};
use sha2::{Digest as _, Sha256};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

static CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[derive(Debug, serde::Deserialize)]
struct Index {
    worlds: Vec<World>,
}

#[derive(Debug, Hash, serde::Deserialize)]
struct World {
    name: String,
    display_name: String,
    #[serde(default)]
    tags: Vec<Tag>,
    discord: Option<String>,
    default_url: Option<String>,
    default_path_in_zip: Option<PathBuf>,
    versions: BTreeMap<Version, WorldVersion>,
}

#[serde_as]
#[derive(Debug, Hash, serde::Deserialize)]
#[serde(untagged)]
enum WorldVersion {
    Url(#[serde_as(as = "NoneAsEmptyString")] Option<String>),
    Full {
        url: Option<String>,
        path_in_zip: Option<PathBuf>,
        #[serde(rename = "as")]
        as_version: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
enum Tag {
    #[serde(rename = "ad")]
    AfterDark,
}

type Cache = BTreeMap<String, CacheEntry>;

#[serde_as]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    info: u64,
    #[serde_as(as = "Hex")]
    file: [u8; 32],
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct WorldSchema {
    name: String,
    game: String,
    version: Version,
    hidden: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    sane_version: Option<Version>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<Tag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wiki: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discord: Option<String>,
}

fn main() -> Result<()> {
    println!("reading index.toml");
    let toml_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "index.toml".into());
    let toml_text = fs::read_to_string(toml_path)?;
    let toml: Index = toml::from_str(&toml_text)?;

    let mut cache: Cache = serde_json::from_str(
        &fs::read_to_string("custom_worlds/cache.json").unwrap_or_else(|_| "{}".into()),
    )
    .context("reading world cache")?;

    fs::create_dir_all("custom_worlds")?;
    _ = fs::create_dir_all("Players");
    for world in &toml.worlds {
        println!("downloading {}", world.name);
        download_world(world, &mut cache)
            .with_context(|| format!("downloading {}.apworld", world.name))?;
    }

    println!("generating schema");
    Command::new("python")
        .arg("GenerateOptionSchema.py")
        .status()?;

    println!("creating index.json");
    let mut index = vec![];
    for entry in fs::read_dir("schema").context("reading schema dir")? {
        let entry = entry?;
        if entry
            .path()
            .file_name()
            .is_some_and(|f| f.to_string_lossy().ends_with(".json") && f != "index.json")
        {
            let mut schema: WorldSchema = serde_json::from_reader(BufReader::new(
                File::open(entry.path())
                    .with_context(|| format!("opening {}", entry.path().display()))?,
            ))
            .with_context(|| format!("reading {}", entry.path().display()))?;
            let info = toml.worlds.iter().find(|w| w.name == schema.name);
            schema.sane_version = info
                .and_then(|w| w.versions.last_key_value())
                .map(|entry| entry.0.clone());
            schema.display_name = info.map(|w| w.display_name.clone());
            schema.tags = info.map(|w| w.tags.clone()).unwrap_or_default();
            schema.wiki = info.map(|w| {
                format!(
                    "https://archipelago.miraheze.org/wiki/{}",
                    w.display_name.replace(' ', "_")
                )
            });
            schema.discord = info.and_then(|w| w.discord.clone());
            index.push(schema);
        }
    }
    index.sort_unstable_by_key(|w| {
        let game_sort = w.display_name.as_ref().unwrap_or(&w.game).to_lowercase();
        let game_sort = game_sort.strip_prefix("the ").unwrap_or(&game_sort);
        let game_sort = game_sort.strip_prefix("an ").unwrap_or(game_sort);
        let game_sort = game_sort.strip_prefix("a ").unwrap_or(game_sort);
        game_sort.to_owned()
    });
    serde_json::to_writer(
        File::create("schema/index.json").context("creating index.json")?,
        &index,
    )
    .context("writing index.json")?;

    Ok(())
}

fn hash<T: Hash>(t: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    t.hash(&mut hasher);
    hasher.finish()
}

fn hash_file(file: &mut File) -> Result<[u8; 32]> {
    let mut hasher = IoWrapper(Sha256::new());
    std::io::copy(file, &mut hasher).context("computing hash of file")?;
    Ok(hasher.0.finalize().0)
}

fn download_world(world: &World, cache: &mut Cache) -> Result<()> {
    let filename = format!("custom_worlds/{}.apworld", world.name);
    let world_hash = hash(world);
    // don't cache if the world has only a single 0.0.0 version, as that
    // indicates unversioned, "latest-only" links
    let should_cache = !(world.versions.len() == 1
        && matches!(world.versions.first_key_value(), Some((v, _)) if *v == Version::new(0, 0, 0)));
    if should_cache
        && let Some(hashes) = cache.get(&world.name)
        && hashes.info == world_hash
        && let Ok(mut file) = File::open(&filename)
    {
        let file_hash = hash_file(&mut file)?;
        if file_hash == hashes.file {
            println!("..skipping");
            return Ok(());
        }
    }

    let (version, version_info) = world
        .versions
        .last_key_value()
        .context("at least one version is required")?;
    let url = match version_info {
        WorldVersion::Url(Some(url)) | WorldVersion::Full { url: Some(url), .. } => url.clone(),
        WorldVersion::Url(None) => world
            .default_url
            .as_ref()
            .context("default_url must be set when version-specific url is missing")?
            .replace("{{version}}", &version.to_string()),
        WorldVersion::Full {
            url: None,
            as_version,
            ..
        } => world
            .default_url
            .as_ref()
            .context("default_url must be set when version-specific url is missing")?
            .replace(
                "{{version}}",
                &as_version.clone().unwrap_or_else(|| version.to_string()),
            ),
    };

    let mut resp = CLIENT.get(url).send()?.error_for_status()?;
    const MAX_SPOOL_SIZE: usize = 20 * 1024 * 1024; // 20 MiB
    let mut download = tempfile::spooled_tempfile_in(MAX_SPOOL_SIZE, ".");
    std::io::copy(&mut resp, &mut download).context("downloading world")?;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&filename)?;

    match (&world.default_path_in_zip, version_info) {
        (
            _,
            WorldVersion::Full {
                path_in_zip: Some(path),
                ..
            },
        )
        | (Some(path), _) => {
            let mut zip = ZipArchive::new(&mut download)?;
            let mut zipped_file = zip
                .by_path(path)
                .context("opening apworld inside zip file")?;
            let mut world_file = tempfile::spooled_tempfile_in(MAX_SPOOL_SIZE, ".");
            std::io::copy(&mut zipped_file, &mut world_file)?;
            fix_zip(&mut world_file, &mut file)?;
        }
        _ => fix_zip(&mut download, &mut file)?,
    }

    file.seek(SeekFrom::Start(0))?;
    if should_cache {
        let file_hash = hash_file(&mut file)?;
        cache.insert(
            world.name.clone(),
            CacheEntry {
                info: world_hash,
                file: file_hash,
            },
        );
        _ = save_cache(cache);
    }

    Ok(())
}

fn save_cache(cache: &Cache) -> Result<()> {
    let file = File::create("custom_worlds/cache.json")?;
    serde_json::to_writer(file, cache)?;
    Ok(())
}

/// Fixes zip archives with invalid backslash path separators to use forward slashes.
fn fix_zip(input: impl Read + Seek, output: impl Write + Seek) -> Result<()> {
    let mut src = ZipArchive::new(input)?;
    let mut dst = ZipWriter::new(output);

    for i in 0..src.len() {
        let mut file = src.by_index(i)?;

        // fix path separators
        let mut filename = file.name().replace('\\', "/");
        if file.is_dir() && !filename.ends_with('/') {
            filename.push('/');
        }

        // copy metadata
        let mut options = SimpleFileOptions::default().compression_method(file.compression());
        if let Some(modified) = file.last_modified() {
            options = options.last_modified_time(modified);
        }
        if let Some(mode) = file.unix_mode() {
            options = options.unix_permissions(mode);
        }

        // write file
        if file.is_dir() {
            dst.add_directory(&filename, options)?;
        } else {
            dst.start_file(&filename, options)?;
            std::io::copy(&mut file, &mut dst)?;
        }
    }

    dst.finish()?;
    Ok(())
}
