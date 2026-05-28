use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::BufReader,
    process::Command,
    sync::LazyLock,
};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use semver::Version;

static CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[derive(Debug, serde::Deserialize)]
struct Index {
    worlds: Vec<World>,
}

#[derive(Debug, serde::Deserialize)]
struct World {
    name: String,
    display_name: String,
    #[serde(default)]
    tags: Vec<Tag>,
    discord: Option<String>,
    default_url: Option<String>,
    versions: BTreeMap<Version, String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
enum Tag {
    #[serde(rename = "ad")]
    AfterDark,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct WorldSchema {
    name: String,
    game: String,
    version: Version,
    hidden: bool,

    #[serde(default)]
    display_name: String,
    #[serde(default)]
    tags: Vec<Tag>,
    #[serde(default)]
    wiki: String,
    discord: Option<String>,
}

fn main() -> Result<()> {
    println!("reading index.toml");
    let toml_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "index.toml".into());
    let toml_text = fs::read_to_string(toml_path)?;
    let toml: Index = toml::from_str(&toml_text)?;

    println!("downloading worlds");
    fs::create_dir_all("custom_worlds")?;
    for world in &toml.worlds {
        println!("downloading {}", world.name);
        download_world(world).with_context(|| format!("downloading {}.apworld", world.name))?;
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
            schema.display_name = info.map(|w| w.display_name.clone()).unwrap_or_default();
            schema.tags = info.map(|w| w.tags.clone()).unwrap_or_default();
            schema.wiki = info
                .map(|w| {
                    format!(
                        "https://archipelago.miraheze.org/wiki/{}",
                        w.display_name.replace(' ', "_")
                    )
                })
                .unwrap_or_default();
            schema.discord = info.and_then(|w| w.discord.clone());
            index.push(schema);
        }
    }
    index.sort_unstable_by_key(|w| w.name.clone());
    serde_json::to_writer(
        File::create("schema/index.json").context("creating index.json")?,
        &index,
    )
    .context("writing index.json")?;

    Ok(())
}

fn download_world(world: &World) -> Result<()> {
    let (version, version_url) = world
        .versions
        .last_key_value()
        .context("at least one version is required")?;
    let url = match version_url.as_str() {
        "" => world
            .default_url
            .as_ref()
            .context("default_url must be set when version-specific url is missing")?
            .replace("{{version}}", &version.to_string()),
        _ => version_url.clone(),
    };
    let mut resp = CLIENT.get(url).send()?.error_for_status()?;
    let mut file = File::create(format!("custom_worlds/{}.apworld", world.name))?;
    std::io::copy(&mut resp, &mut file)?;

    Ok(())
}
