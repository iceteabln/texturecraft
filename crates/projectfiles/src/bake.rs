//! Bake a `.sbsar` through the optional host executable.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

pub const MIN_BAKE_SIZE: u32 = 256;
pub const MAX_BAKE_SIZE: u32 = 2048;

#[derive(Clone, Debug, Deserialize)]
pub struct BakeManifest {
    pub graph: String,
    pub graph_index: usize,
    pub size: u32,
    pub graphs: Vec<GraphEntry>,
    pub outputs: Vec<BakedOutput>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GraphEntry {
    pub index: usize,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BakedOutput {
    pub file: String,
    pub identifier: String,
}

/// Power of two in the host's accepted range.
pub fn clamp_bake_size(size_px: u32) -> u32 {
    let size = size_px.clamp(MIN_BAKE_SIZE, MAX_BAKE_SIZE);
    size.next_power_of_two().min(MAX_BAKE_SIZE).max(MIN_BAKE_SIZE)
}

/// Locate the bake host. `TEXTURECRAFT_SBSAR_HOST` wins, then a copy beside this executable.
pub fn resolve_host() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TEXTURECRAFT_SBSAR_HOST") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join("sbsar-bake.exe"), dir.join("sbsar-bake"), dir.join("subcraft-sbsar-bake.exe"), dir.join("subcraft-sbsar-bake")] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn sdk_bin_dir(host: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TEXTURECRAFT_SDK_BIN") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }
    let parent = host.parent()?;
    if parent.join("substance_sse2_blend.dll").is_file() {
        return Some(parent.to_path_buf());
    }
    None
}

/// Bake graph `graph_index` of `sbsar` into `out_dir` (`manifest.json` plus map files).
pub fn bake_sbsar(sbsar: &Path, out_dir: &Path, size_px: u32, graph_index: usize) -> Result<BakeManifest, String> {
    let host = resolve_host().ok_or_else(|| "Bake host not found. Set TEXTURECRAFT_SBSAR_HOST to the bake executable.".to_string())?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let size = clamp_bake_size(size_px);
    let mut cmd = Command::new(&host);
    cmd.arg(sbsar).arg(out_dir).arg(size.to_string()).arg(graph_index.to_string());
    if let Some(bin) = sdk_bin_dir(&host) {
        if let Ok(existing) = std::env::var("PATH") {
            let merged = format!("{};{}", bin.display(), existing);
            cmd.env("PATH", merged);
        } else {
            cmd.env("PATH", bin.as_os_str());
        }
    }
    let output = cmd.output().map_err(|e| format!("failed to run {}: {e}", host.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("archive bake failed (exit {:?})\n{stderr}{stdout}", output.status.code()));
    }
    let manifest_path = out_dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))
}
