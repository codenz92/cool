use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DependencySpec {
    pub name: String,
    #[allow(dead_code)]
    pub version: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
struct DependencyRoots {
    name: String,
    roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ModuleResolver {
    local_roots: Vec<PathBuf>,
    dependency_roots: Vec<DependencyRoots>,
}

#[derive(Debug, Clone)]
pub struct CoolProject {
    pub root: PathBuf,
    #[allow(dead_code)]
    pub manifest_path: PathBuf,
    pub name: String,
    pub version: String,
    pub main: String,
    pub output: Option<String>,
    pub sources: Vec<String>,
    pub dependencies: Vec<DependencySpec>,
}

fn canonical_or_path(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn find_manifest_dir(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = canonical_or_path(start_dir.to_path_buf());
    loop {
        if dir.join("cool.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn module_candidates(root: &Path, file_path: &str) -> [PathBuf; 2] {
    [
        root.join(format!("{file_path}.cool")),
        root.join(file_path).join("__init__.cool"),
    ]
}

fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        if seen.insert(path.clone()) {
            out.push(path);
        }
    }
    out
}

impl CoolProject {
    pub fn discover(start_dir: &Path) -> Result<Option<Self>, String> {
        match find_manifest_dir(start_dir) {
            Some(dir) => Self::from_manifest_path(&dir.join("cool.toml")).map(Some),
            None => Ok(None),
        }
    }

    pub fn from_manifest_path(manifest_path: &Path) -> Result<Self, String> {
        let manifest_src = fs::read_to_string(manifest_path)
            .map_err(|e| format!("cool.toml: cannot read '{}': {e}", manifest_path.display()))?;
        let parsed: toml::Value = manifest_src
            .parse()
            .map_err(|e: toml::de::Error| format!("cool.toml parse error: {}", e.message()))?;
        let root = parsed
            .as_table()
            .ok_or_else(|| "cool.toml: root must be a table".to_string())?;
        let project = root.get("project").and_then(toml::Value::as_table);

        let field =
            |key: &str| -> Option<&toml::Value> { project.and_then(|table| table.get(key)).or_else(|| root.get(key)) };
        let opt_string = |key: &str| -> Result<Option<String>, String> {
            match field(key) {
                None => Ok(None),
                Some(toml::Value::String(s)) => Ok(Some(s.clone())),
                Some(other) => Err(format!(
                    "cool.toml: field '{}' must be a string, got {}",
                    key,
                    other.type_str()
                )),
            }
        };
        let opt_string_list = |key: &str| -> Result<Vec<String>, String> {
            match field(key) {
                None => Ok(Vec::new()),
                Some(toml::Value::Array(items)) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        match item {
                            toml::Value::String(s) => out.push(s.clone()),
                            other => {
                                return Err(format!(
                                    "cool.toml: field '{}' must be an array of strings, got {}",
                                    key,
                                    other.type_str()
                                ))
                            }
                        }
                    }
                    Ok(out)
                }
                Some(other) => Err(format!(
                    "cool.toml: field '{}' must be an array of strings, got {}",
                    key,
                    other.type_str()
                )),
            }
        };

        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| format!("cool.toml: invalid manifest path '{}'", manifest_path.display()))?;
        let manifest_dir = canonical_or_path(manifest_dir.to_path_buf());

        let mut dependencies = Vec::new();
        if let Some(value) = root
            .get("dependencies")
            .or_else(|| project.and_then(|table| table.get("dependencies")))
        {
            match value {
                toml::Value::Table(table) => {
                    for (name, spec) in table {
                        match spec {
                            toml::Value::String(path) => dependencies.push(DependencySpec {
                                name: name.clone(),
                                version: None,
                                path: PathBuf::from(path),
                            }),
                            toml::Value::Table(dep_table) => {
                                let path = match dep_table.get("path") {
                                    Some(toml::Value::String(path)) => PathBuf::from(path),
                                    Some(other) => {
                                        return Err(format!(
                                            "cool.toml: dependency '{}' field 'path' must be a string, got {}",
                                            name,
                                            other.type_str()
                                        ))
                                    }
                                    None => PathBuf::from("deps").join(name),
                                };
                                let version = match dep_table.get("version") {
                                    Some(toml::Value::String(version)) => Some(version.clone()),
                                    Some(other) => {
                                        return Err(format!(
                                            "cool.toml: dependency '{}' field 'version' must be a string, got {}",
                                            name,
                                            other.type_str()
                                        ))
                                    }
                                    None => None,
                                };
                                dependencies.push(DependencySpec {
                                    name: name.clone(),
                                    version,
                                    path,
                                });
                            }
                            other => {
                                return Err(format!(
                                    "cool.toml: dependency '{}' must be a string path or table, got {}",
                                    name,
                                    other.type_str()
                                ))
                            }
                        }
                    }
                }
                toml::Value::Array(items) => {
                    for item in items {
                        match item {
                            toml::Value::String(name) => dependencies.push(DependencySpec {
                                name: name.clone(),
                                version: None,
                                path: PathBuf::from("deps").join(name),
                            }),
                            other => {
                                return Err(format!(
                                    "cool.toml: dependencies array must contain strings, got {}",
                                    other.type_str()
                                ))
                            }
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "cool.toml: field 'dependencies' must be a table or array of strings, got {}",
                        other.type_str()
                    ))
                }
            }
        }

        Ok(CoolProject {
            root: manifest_dir.clone(),
            manifest_path: manifest_dir.join("cool.toml"),
            name: opt_string("name")?.unwrap_or_else(|| "project".to_string()),
            version: opt_string("version")?.unwrap_or_else(|| "0.1.0".to_string()),
            main: opt_string("main")?.ok_or("cool.toml: missing required key 'main'")?,
            output: opt_string("output")?,
            sources: opt_string_list("sources")?,
            dependencies,
        })
    }

    pub fn output_name(&self) -> &str {
        self.output.as_deref().unwrap_or(&self.name)
    }

    pub fn main_path(&self) -> PathBuf {
        self.root.join(&self.main)
    }

    pub fn local_module_roots(&self) -> Result<Vec<PathBuf>, String> {
        let raw_roots = if self.sources.is_empty() {
            let main_path = Path::new(&self.main);
            let parent = main_path.parent().unwrap_or(Path::new("."));
            vec![self.root.join(parent)]
        } else {
            self.sources.iter().map(|source| self.root.join(source)).collect()
        };

        let mut out = Vec::new();
        for root in raw_roots {
            if !root.exists() {
                return Err(format!(
                    "cool.toml: source directory '{}' does not exist",
                    root.display()
                ));
            }
            if !root.is_dir() {
                return Err(format!(
                    "cool.toml: source path '{}' is not a directory",
                    root.display()
                ));
            }
            out.push(canonical_or_path(root));
        }
        Ok(unique_paths(out))
    }
}

impl ModuleResolver {
    pub fn local_only(source_dir: PathBuf) -> Self {
        Self {
            local_roots: vec![canonical_or_path(source_dir)],
            dependency_roots: Vec::new(),
        }
    }

    pub fn discover_for_script(source_dir: &Path) -> Result<Self, String> {
        match CoolProject::discover(source_dir)? {
            Some(project) => Self::from_project(&project),
            None => Ok(Self::local_only(source_dir.to_path_buf())),
        }
    }

    pub fn resolve_module(&self, current_source_dir: &Path, name: &str) -> Option<PathBuf> {
        let file_path = name.replace('.', "/");
        for candidate in module_candidates(current_source_dir, &file_path) {
            if candidate.exists() {
                return Some(canonical_or_path(candidate));
            }
        }

        for root in &self.local_roots {
            for candidate in module_candidates(root, &file_path) {
                if candidate.exists() {
                    return Some(canonical_or_path(candidate));
                }
            }
        }

        let segments: Vec<&str> = name.split('.').collect();
        let dep_name = segments.first().copied()?;
        let suffix = segments[1..].join("/");
        for dep in &self.dependency_roots {
            if dep.name != dep_name {
                continue;
            }
            for root in &dep.roots {
                let candidates = if suffix.is_empty() {
                    vec![root.join("__init__.cool")]
                } else {
                    module_candidates(root, &suffix).into_iter().collect()
                };
                for candidate in candidates {
                    if candidate.exists() {
                        return Some(canonical_or_path(candidate));
                    }
                }
            }
        }
        None
    }

    fn from_project(project: &CoolProject) -> Result<Self, String> {
        let local_roots = project.local_module_roots()?;
        let mut dependency_roots = Vec::new();
        let mut visited = HashSet::new();
        collect_dependency_roots(project, &mut dependency_roots, &mut visited)?;
        Ok(Self {
            local_roots,
            dependency_roots,
        })
    }
}

fn dependency_roots_for_path(dep_root: &Path) -> Result<Vec<PathBuf>, String> {
    let src_root = dep_root.join("src");
    let mut roots = Vec::new();
    if src_root.exists() && src_root.is_dir() {
        roots.push(canonical_or_path(src_root));
    }
    if dep_root.exists() && dep_root.is_dir() {
        roots.push(canonical_or_path(dep_root.to_path_buf()));
    }
    if roots.is_empty() {
        return Err(format!(
            "cool.toml: dependency path '{}' does not exist or has no source roots",
            dep_root.display()
        ));
    }
    Ok(unique_paths(roots))
}

fn collect_dependency_roots(
    project: &CoolProject,
    out: &mut Vec<DependencyRoots>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    for dep in &project.dependencies {
        let dep_root = canonical_or_path(project.root.join(&dep.path));
        if !visited.insert(dep_root.clone()) {
            continue;
        }
        if !dep_root.exists() {
            return Err(format!(
                "cool.toml: dependency '{}' path '{}' does not exist",
                dep.name,
                dep_root.display()
            ));
        }

        let dep_manifest = dep_root.join("cool.toml");
        if dep_manifest.exists() {
            let dep_project = CoolProject::from_manifest_path(&dep_manifest)?;
            out.push(DependencyRoots {
                name: dep.name.clone(),
                roots: dep_project.local_module_roots()?,
            });
            collect_dependency_roots(&dep_project, out, visited)?;
        } else {
            out.push(DependencyRoots {
                name: dep.name.clone(),
                roots: dependency_roots_for_path(&dep_root)?,
            });
        }
    }
    Ok(())
}
