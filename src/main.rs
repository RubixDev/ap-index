use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::BufReader,
    process::{Command, Stdio},
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
    home: String,
    default_url: Option<String>,
    versions: BTreeMap<Version, String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct WorldSchema {
    name: String,
    game: String,
    version: Version,
    hidden: bool,
    // and other unimportant fields
}

fn main() -> Result<()> {
    println!("reading index.toml");
    let toml_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "index.toml".into());
    let toml_text = fs::read_to_string(toml_path)?;
    let index: Index = toml::from_str(&toml_text)?;

    println!("downloading worlds");
    fs::create_dir_all("custom_worlds")?;
    for world in index.worlds {
        println!("downloading {}", world.name);
        download_world(&world).with_context(|| format!("downloading {}.apworld", world.name))?;
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
            let schema: WorldSchema = serde_json::from_reader(BufReader::new(
                File::open(entry.path())
                    .with_context(|| format!("opening {}", entry.path().display()))?,
            ))
            .with_context(|| format!("reading {}", entry.path().display()))?;
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
