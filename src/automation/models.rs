use std::fmt;
use std::path::PathBuf;
use std::{fs, path::Path};
use anyhow::anyhow;
use tracing::{debug, info};
use url::Url;

/// Return the path to the directory in which cached models etc. should be
/// saved.
fn cache_dir() -> Result<PathBuf, anyhow::Error> {
    let mut cache_dir: PathBuf =
        home::home_dir().ok_or(anyhow!("Failed to determine home directory"))?;
    cache_dir.push(".cache");
    cache_dir.push("isotope-ocr");

    fs::create_dir_all(&cache_dir)?;

    Ok(cache_dir)
}

/// Extract the last path segment from a URL.
///
/// eg. "https://models.com/text-detection.rten" => "text-detection.rten".
fn filename_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let path = Path::new(parsed.path());
    path.file_name()
        .and_then(|f| f.to_str())
        .map(|s| s.to_string())
}

/// Download a file from `url` to a local cache, if not already fetched, and
/// return the path to the local file.
pub fn download_file(url: &str, filename: Option<&str>) -> Result<PathBuf, anyhow::Error> {
    let cache_dir = cache_dir()?;
    let filename = match filename {
        Some(fname) => fname.to_string(),
        None => filename_from_url(url).ok_or(anyhow!("Could not get destination filename"))?,
    };
    let file_path = cache_dir.join(filename);
    if file_path.exists() {
        debug!("Using cached model: {:?}", file_path);
        return Ok(file_path);
    }

    info!("Downloading OCR model from {}...", url);

    let response = ureq::get(url).call()?;
    let mut body = response.into_body();
    let buf = body.read_to_vec()?;

    fs::write(&file_path, &buf)?;
    info!("Downloaded OCR model to: {:?}", file_path);

    Ok(file_path)
}

/// Location that a model can be loaded from.
#[derive(Clone)]
pub enum ModelSource {
    /// Load model from an HTTP(S) URL.
    Url(String),

    /// Load model from a local file path.
    Path(String),
}

impl fmt::Display for ModelSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ModelSource::Url(url) => url,
                ModelSource::Path(path) => path,
            }
        )
    }
}

/// Load a model from a given source.
///
/// If the source is a URL, the model will be downloaded and cached locally if
/// needed.
pub fn load_model(source: ModelSource) -> Result<rten::Model, anyhow::Error> {
    let model_path = match source {
        ModelSource::Url(url) => download_file(&url, None)?,
        ModelSource::Path(path) => path.into(),
    };
    let model = rten::Model::load_file(model_path)?;
    Ok(model)
}