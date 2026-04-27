use crate::tooling;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn canonical_or_path(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Clone)]
pub struct BuildFingerprintInput {
    pub entry_path: PathBuf,
    pub manifest_path: Option<PathBuf>,
    pub extra_files: Vec<PathBuf>,
    pub identity: Vec<(String, String)>,
}

impl BuildFingerprintInput {
    pub fn compute(&self) -> Result<String, String> {
        let mut hasher = Sha256::new();
        hasher.update(b"cool-build-cache-v1");

        let mut identity = self.identity.clone();
        identity.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in &identity {
            hasher.update(key.as_bytes());
            hasher.update([0]);
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }

        for path in self.input_files()? {
            hasher.update(b"file");
            hasher.update(path_string(&path).as_bytes());
            hasher.update([0]);
            let bytes = fs::read(&path).map_err(|e| format!("build cache: cannot read '{}': {e}", path.display()))?;
            hasher.update(bytes.len().to_le_bytes());
            hasher.update(&bytes);
        }

        Ok(hex_digest(&hasher.finalize()))
    }

    fn input_files(&self) -> Result<Vec<PathBuf>, String> {
        let mut out = Vec::new();
        let mut push_unique = |path: PathBuf| {
            if !out.contains(&path) {
                out.push(path);
            }
        };

        match tooling::build_module_graph(&self.entry_path) {
            Ok(graph) => {
                for module in graph.modules {
                    push_unique(canonical_or_path(PathBuf::from(module.path)));
                }
            }
            Err(_) => push_unique(canonical_or_path(self.entry_path.clone())),
        }

        if let Some(manifest) = &self.manifest_path {
            if manifest.exists() {
                push_unique(canonical_or_path(manifest.clone()));
            }
        }

        for path in &self.extra_files {
            if path.exists() {
                push_unique(canonical_or_path(path.clone()));
            }
        }

        out.sort();
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct BuildCache {
    root: PathBuf,
    fingerprint: String,
}

impl BuildCache {
    pub fn new(root: PathBuf, fingerprint: String) -> Self {
        Self { root, fingerprint }
    }

    pub fn restore(&self, outputs: &[PathBuf]) -> Result<bool, String> {
        let entry = self.entry_dir();
        if !entry.exists() {
            return Ok(false);
        }

        let mut cached_paths = Vec::with_capacity(outputs.len());
        for output in outputs {
            let file_name = output
                .file_name()
                .ok_or_else(|| format!("build cache: invalid output path '{}'", output.display()))?;
            let cached = entry.join(file_name);
            if !cached.exists() {
                return Ok(false);
            }
            cached_paths.push((cached, output.clone()));
        }

        for (cached, output) in cached_paths {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("build cache: cannot create '{}': {e}", parent.display()))?;
            }
            fs::copy(&cached, &output).map_err(|e| {
                format!(
                    "build cache: cannot restore '{}' from '{}': {e}",
                    output.display(),
                    cached.display()
                )
            })?;
        }

        Ok(true)
    }

    pub fn store(&self, outputs: &[PathBuf]) -> Result<(), String> {
        let entry = self.entry_dir();
        if entry.exists() {
            fs::remove_dir_all(&entry).map_err(|e| format!("build cache: cannot clear '{}': {e}", entry.display()))?;
        }
        fs::create_dir_all(&entry).map_err(|e| format!("build cache: cannot create '{}': {e}", entry.display()))?;

        for output in outputs {
            let file_name = output
                .file_name()
                .ok_or_else(|| format!("build cache: invalid output path '{}'", output.display()))?;
            if !output.exists() {
                return Err(format!(
                    "build cache: expected build output '{}' to exist before caching",
                    output.display()
                ));
            }
            fs::copy(output, entry.join(file_name)).map_err(|e| {
                format!(
                    "build cache: cannot store '{}' in '{}': {e}",
                    output.display(),
                    entry.display()
                )
            })?;
        }

        Ok(())
    }

    fn entry_dir(&self) -> PathBuf {
        self.root.join(&self.fingerprint)
    }
}
