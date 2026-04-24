use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub enum DependencySource {
    Path {
        path: PathBuf,
    },
    Git {
        git: String,
        branch: Option<String>,
        tag: Option<String>,
        rev: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct DependencySpec {
    pub name: String,
    pub version: Option<String>,
    pub source: DependencySource,
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
    pub manifest_path: PathBuf,
    pub name: String,
    pub version: String,
    pub main: String,
    pub output: Option<String>,
    pub sources: Vec<String>,
    pub dependencies: Vec<DependencySpec>,
}

#[derive(Debug, Clone)]
pub struct LockfileDependency {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub resolved_path: String,
    pub version: Option<String>,
    pub git: Option<String>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub rev: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoolLockfile {
    pub project_name: String,
    pub project_version: String,
    pub dependencies: Vec<LockfileDependency>,
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

fn parse_string_field(
    value: Option<&toml::Value>,
    field_name: &str,
    context: &str,
) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(format!(
            "{context}: field '{field_name}' must be a string, got {}",
            other.type_str()
        )),
    }
}

fn validate_git_selector(
    context: &str,
    name: &str,
    branch: &Option<String>,
    tag: &Option<String>,
    rev: &Option<String>,
) -> Result<(), String> {
    let selector_count = usize::from(branch.is_some()) + usize::from(tag.is_some()) + usize::from(rev.is_some());
    if selector_count > 1 {
        return Err(format!(
            "{context}: dependency '{name}' may specify at most one of 'branch', 'tag', or 'rev'"
        ));
    }
    Ok(())
}

fn is_remote_git_source(source: &str) -> bool {
    source.contains("://") || source.starts_with("git@")
}

fn shell_join(args: &[String]) -> String {
    args.join(" ")
}

fn run_git_command(cwd: Option<&Path>, args: &[String]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", shell_join(args)))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "git command failed".to_string()
        };
        Err(format!("git {}: {details}", shell_join(args)))
    }
}

fn escape_toml_string(text: &str) -> String {
    format!("{text:?}")
}

fn manifest_dependency_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn diff_paths(target: &Path, base: &Path) -> Option<PathBuf> {
    let target_components: Vec<Component<'_>> = target.components().collect();
    let base_components: Vec<Component<'_>> = base.components().collect();

    if target_components.is_empty() || base_components.is_empty() {
        return None;
    }

    if target_components.first() != base_components.first() {
        return None;
    }

    let mut common = 0usize;
    while common < target_components.len()
        && common < base_components.len()
        && target_components[common] == base_components[common]
    {
        common += 1;
    }

    let mut out = PathBuf::new();
    for _ in common..base_components.len() {
        out.push("..");
    }
    for component in &target_components[common..] {
        out.push(component.as_os_str());
    }

    if out.as_os_str().is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(out)
    }
}

impl DependencySpec {
    pub fn path(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            version: None,
            source: DependencySource::Path { path: path.into() },
        }
    }

    pub fn git(name: impl Into<String>, git: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            source: DependencySource::Git {
                git: git.into(),
                branch: None,
                tag: None,
                rev: None,
            },
        }
    }

    pub fn manifest_value(&self) -> toml::Value {
        let mut table = toml::map::Map::new();
        match &self.source {
            DependencySource::Path { path } => {
                table.insert("path".to_string(), toml::Value::String(manifest_dependency_path(path)));
            }
            DependencySource::Git {
                git,
                branch,
                tag,
                rev,
            } => {
                table.insert("git".to_string(), toml::Value::String(git.clone()));
                if let Some(branch) = branch {
                    table.insert("branch".to_string(), toml::Value::String(branch.clone()));
                }
                if let Some(tag) = tag {
                    table.insert("tag".to_string(), toml::Value::String(tag.clone()));
                }
                if let Some(rev) = rev {
                    table.insert("rev".to_string(), toml::Value::String(rev.clone()));
                }
            }
        }
        if let Some(version) = &self.version {
            table.insert("version".to_string(), toml::Value::String(version.clone()));
        }
        toml::Value::Table(table)
    }

    pub fn resolved_root(&self, project_root: &Path) -> PathBuf {
        match &self.source {
            DependencySource::Path { path } => canonical_or_path(project_root.join(path)),
            DependencySource::Git { .. } => canonical_or_path(project_root.join(".cool").join("deps").join(&self.name)),
        }
    }

    fn git_clone_source(&self, project_root: &Path) -> Option<String> {
        match &self.source {
            DependencySource::Git { git, .. } => {
                if is_remote_git_source(git) {
                    Some(git.clone())
                } else {
                    let raw = Path::new(git);
                    let path = if raw.is_absolute() {
                        raw.to_path_buf()
                    } else {
                        project_root.join(raw)
                    };
                    Some(path.to_string_lossy().to_string())
                }
            }
            DependencySource::Path { .. } => None,
        }
    }

    fn install_hint(&self, project_root: &Path) -> String {
        match &self.source {
            DependencySource::Git { .. } => format!(
                "git dependency '{}' is not installed at '{}'. Run `cool install`.",
                self.name,
                self.resolved_root(project_root).display()
            ),
            DependencySource::Path { .. } => format!(
                "dependency '{}' path '{}' does not exist",
                self.name,
                self.resolved_root(project_root).display()
            ),
        }
    }
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
            parse_string_field(field(key), key, "cool.toml")
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
                                source: DependencySource::Path {
                                    path: PathBuf::from(path),
                                },
                            }),
                            toml::Value::Table(dep_table) => {
                                let version = parse_string_field(dep_table.get("version"), "version", "cool.toml")?;
                                let git = parse_string_field(dep_table.get("git"), "git", "cool.toml")?;
                                let branch = parse_string_field(dep_table.get("branch"), "branch", "cool.toml")?;
                                let tag = parse_string_field(dep_table.get("tag"), "tag", "cool.toml")?;
                                let rev = parse_string_field(dep_table.get("rev"), "rev", "cool.toml")?;

                                if let Some(git) = git {
                                    if dep_table.get("path").is_some() {
                                        return Err(format!(
                                            "cool.toml: dependency '{}' may not specify both 'git' and 'path'",
                                            name
                                        ));
                                    }
                                    validate_git_selector("cool.toml", name, &branch, &tag, &rev)?;
                                    dependencies.push(DependencySpec {
                                        name: name.clone(),
                                        version,
                                        source: DependencySource::Git {
                                            git,
                                            branch,
                                            tag,
                                            rev,
                                        },
                                    });
                                } else {
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
                                    dependencies.push(DependencySpec {
                                        name: name.clone(),
                                        version,
                                        source: DependencySource::Path { path },
                                    });
                                }
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
                                source: DependencySource::Path {
                                    path: PathBuf::from("deps").join(name),
                                },
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

    pub fn managed_dir(&self) -> PathBuf {
        self.root.join(".cool")
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.root.join("cool.lock")
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

impl CoolLockfile {
    pub fn read(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }

        let src = fs::read_to_string(path)
            .map_err(|e| format!("cool.lock: cannot read '{}': {e}", path.display()))?;
        let parsed: toml::Value = src
            .parse()
            .map_err(|e: toml::de::Error| format!("cool.lock parse error: {}", e.message()))?;
        let root = parsed
            .as_table()
            .ok_or_else(|| "cool.lock: root must be a table".to_string())?;

        let project = root.get("project").and_then(toml::Value::as_table);
        let project_name = project
            .and_then(|table| table.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or("project")
            .to_string();
        let project_version = project
            .and_then(|table| table.get("version"))
            .and_then(toml::Value::as_str)
            .unwrap_or("0.1.0")
            .to_string();

        let mut dependencies = Vec::new();
        if let Some(items) = root.get("dependency").and_then(toml::Value::as_array) {
            for item in items {
                let table = item
                    .as_table()
                    .ok_or_else(|| "cool.lock: [[dependency]] entries must be tables".to_string())?;
                let name = table
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| "cool.lock: dependency entry missing string field 'name'".to_string())?
                    .to_string();
                let kind = table
                    .get("kind")
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| "cool.lock: dependency entry missing string field 'kind'".to_string())?
                    .to_string();
                let path = table
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let resolved_path = table
                    .get("resolved_path")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let version = table.get("version").and_then(toml::Value::as_str).map(ToOwned::to_owned);
                let git = table.get("git").and_then(toml::Value::as_str).map(ToOwned::to_owned);
                let branch = table.get("branch").and_then(toml::Value::as_str).map(ToOwned::to_owned);
                let tag = table.get("tag").and_then(toml::Value::as_str).map(ToOwned::to_owned);
                let rev = table.get("rev").and_then(toml::Value::as_str).map(ToOwned::to_owned);
                dependencies.push(LockfileDependency {
                    name,
                    kind,
                    path,
                    resolved_path,
                    version,
                    git,
                    branch,
                    tag,
                    rev,
                });
            }
        }

        Ok(Some(Self {
            project_name,
            project_version,
            dependencies,
        }))
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        fs::write(path, self.render()).map_err(|e| format!("cool.lock: cannot write '{}': {e}", path.display()))
    }

    pub fn locked_rev(&self, name: &str) -> Option<&str> {
        self.dependencies
            .iter()
            .rev()
            .find(|dep| dep.name == name && dep.kind == "git")
            .and_then(|dep| dep.rev.as_deref())
    }

    fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("version = 1\n\n");
        out.push_str("[project]\n");
        out.push_str(&format!("name = {}\n", escape_toml_string(&self.project_name)));
        out.push_str(&format!("version = {}\n", escape_toml_string(&self.project_version)));

        for dep in &self.dependencies {
            out.push_str("\n[[dependency]]\n");
            out.push_str(&format!("name = {}\n", escape_toml_string(&dep.name)));
            out.push_str(&format!("kind = {}\n", escape_toml_string(&dep.kind)));
            out.push_str(&format!("path = {}\n", escape_toml_string(&dep.path)));
            out.push_str(&format!(
                "resolved_path = {}\n",
                escape_toml_string(&dep.resolved_path)
            ));
            if let Some(version) = &dep.version {
                out.push_str(&format!("version = {}\n", escape_toml_string(version)));
            }
            if let Some(git) = &dep.git {
                out.push_str(&format!("git = {}\n", escape_toml_string(git)));
            }
            if let Some(branch) = &dep.branch {
                out.push_str(&format!("branch = {}\n", escape_toml_string(branch)));
            }
            if let Some(tag) = &dep.tag {
                out.push_str(&format!("tag = {}\n", escape_toml_string(tag)));
            }
            if let Some(rev) = &dep.rev {
                out.push_str(&format!("rev = {}\n", escape_toml_string(rev)));
            }
        }

        out
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

pub fn normalize_dependency_source_arg(project_root: &Path, current_dir: &Path, source: &str) -> String {
    if is_remote_git_source(source) {
        return source.to_string();
    }

    let raw = Path::new(source);
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        current_dir.join(raw)
    };
    let absolute = canonical_or_path(absolute);
    let project_root = canonical_or_path(project_root.to_path_buf());

    diff_paths(&absolute, &project_root)
        .unwrap_or(absolute)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn add_dependency_to_manifest(manifest_path: &Path, dependency: &DependencySpec) -> Result<(), String> {
    let manifest_src = fs::read_to_string(manifest_path)
        .map_err(|e| format!("cool.toml: cannot read '{}': {e}", manifest_path.display()))?;
    let mut parsed: toml::Value = manifest_src
        .parse()
        .map_err(|e: toml::de::Error| format!("cool.toml parse error: {}", e.message()))?;
    let root = parsed
        .as_table_mut()
        .ok_or_else(|| "cool.toml: root must be a table".to_string())?;
    let deps = root
        .entry("dependencies")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let deps_table = deps
        .as_table_mut()
        .ok_or_else(|| "cool.toml: [dependencies] must be a table".to_string())?;
    deps_table.insert(dependency.name.clone(), dependency.manifest_value());

    let rendered = toml::to_string_pretty(&parsed)
        .map_err(|e| format!("cool.toml: cannot serialize manifest: {e}"))?;
    fs::write(manifest_path, rendered)
        .map_err(|e| format!("cool.toml: cannot write '{}': {e}", manifest_path.display()))
}

pub fn install_dependencies(project: &CoolProject) -> Result<CoolLockfile, String> {
    let existing_lockfile = CoolLockfile::read(&project.lockfile_path())?;
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();
    install_project_dependencies(project, existing_lockfile.as_ref(), &mut dependencies, &mut seen)?;

    let lockfile = CoolLockfile {
        project_name: project.name.clone(),
        project_version: project.version.clone(),
        dependencies,
    };
    if !project.managed_dir().exists() {
        fs::create_dir_all(project.managed_dir())
            .map_err(|e| format!("cool install: cannot create '{}': {e}", project.managed_dir().display()))?;
    }
    lockfile.write(&project.lockfile_path())?;
    Ok(lockfile)
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
        let dep_root = dep.resolved_root(&project.root);
        if !visited.insert(dep_root.clone()) {
            continue;
        }
        if !dep_root.exists() {
            return Err(format!("cool.toml: {}", dep.install_hint(&project.root)));
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

fn install_project_dependencies(
    project: &CoolProject,
    existing_lockfile: Option<&CoolLockfile>,
    out: &mut Vec<LockfileDependency>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    for dep in &project.dependencies {
        let dep_root = materialize_dependency(project, dep, existing_lockfile, out)?;
        let dep_root = canonical_or_path(dep_root);
        if !visited.insert(dep_root.clone()) {
            continue;
        }

        let dep_manifest = dep_root.join("cool.toml");
        if dep_manifest.exists() {
            let dep_project = CoolProject::from_manifest_path(&dep_manifest)?;
            install_project_dependencies(&dep_project, existing_lockfile, out, visited)?;
        }
    }
    Ok(())
}

fn maybe_project_version(root: &Path) -> Result<Option<String>, String> {
    let manifest = root.join("cool.toml");
    if !manifest.exists() {
        return Ok(None);
    }
    let project = CoolProject::from_manifest_path(&manifest)?;
    Ok(Some(project.version))
}

fn materialize_dependency(
    project: &CoolProject,
    dep: &DependencySpec,
    existing_lockfile: Option<&CoolLockfile>,
    out: &mut Vec<LockfileDependency>,
) -> Result<PathBuf, String> {
    match &dep.source {
        DependencySource::Path { path } => {
            let dep_root = dep.resolved_root(&project.root);
            if !dep_root.exists() {
                return Err(format!("cool install: {}", dep.install_hint(&project.root)));
            }
            out.push(LockfileDependency {
                name: dep.name.clone(),
                kind: "path".to_string(),
                path: manifest_dependency_path(path),
                resolved_path: dep_root.to_string_lossy().to_string(),
                version: maybe_project_version(&dep_root)?,
                git: None,
                branch: None,
                tag: None,
                rev: None,
            });
            Ok(dep_root)
        }
        DependencySource::Git {
            git,
            branch,
            tag,
            rev,
        } => {
            let dep_root = dep.resolved_root(&project.root);
            let parent = dep_root
                .parent()
                .ok_or_else(|| format!("cool install: invalid dependency directory '{}'", dep_root.display()))?;
            fs::create_dir_all(parent)
                .map_err(|e| format!("cool install: cannot create '{}': {e}", parent.display()))?;

            if !dep_root.exists() {
                let clone_source = dep
                    .git_clone_source(&project.root)
                    .ok_or_else(|| format!("cool install: dependency '{}' is not a git source", dep.name))?;
                run_git_command(
                    None,
                    &[
                        "clone".to_string(),
                        clone_source,
                        dep_root.to_string_lossy().to_string(),
                    ],
                )?;
            } else if !dep_root.join(".git").exists() {
                return Err(format!(
                    "cool install: managed dependency directory '{}' exists but is not a git checkout",
                    dep_root.display()
                ));
            }

            let locked_rev = existing_lockfile.and_then(|lockfile| lockfile.locked_rev(&dep.name));
            if branch.is_some() || tag.is_some() {
                let _ = run_git_command(
                    Some(&dep_root),
                    &["fetch".to_string(), "--all".to_string(), "--tags".to_string()],
                );
            }

            if let Some(target) = desired_git_ref(branch, tag, rev, locked_rev) {
                run_git_command(
                    Some(&dep_root),
                    &["checkout".to_string(), "--detach".to_string(), target],
                )?;
            }

            let resolved_rev = run_git_command(
                Some(&dep_root),
                &["rev-parse".to_string(), "HEAD".to_string()],
            )?;
            out.push(LockfileDependency {
                name: dep.name.clone(),
                kind: "git".to_string(),
                path: format!(".cool/deps/{}", dep.name),
                resolved_path: dep_root.to_string_lossy().to_string(),
                version: maybe_project_version(&dep_root)?,
                git: Some(git.clone()),
                branch: branch.clone(),
                tag: tag.clone(),
                rev: Some(resolved_rev),
            });
            Ok(dep_root)
        }
    }
}

fn desired_git_ref(
    branch: &Option<String>,
    tag: &Option<String>,
    rev: &Option<String>,
    locked_rev: Option<&str>,
) -> Option<String> {
    if let Some(rev) = rev {
        return Some(rev.clone());
    }
    if let Some(tag) = tag {
        return Some(format!("refs/tags/{tag}"));
    }
    if let Some(branch) = branch {
        return Some(format!("origin/{branch}"));
    }
    locked_rev.map(ToOwned::to_owned)
}
