use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMode {
    Application,
    Library,
    Workspace,
}

impl AnalysisMode {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "application" => Ok(Self::Application),
            "library" => Ok(Self::Library),
            "workspace" => Ok(Self::Workspace),
            _ => Err(format!(
                "invalid --analysis-mode '{raw}'; expected application|library|workspace"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoTargetKind {
    Bin,
    Lib,
    Example,
    Test,
    Bench,
}

impl CargoTargetKind {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "bin" => Ok(Self::Bin),
            "lib" => Ok(Self::Lib),
            "example" => Ok(Self::Example),
            "test" => Ok(Self::Test),
            "bench" => Ok(Self::Bench),
            _ => Err(format!(
                "invalid --cargo-kind '{raw}'; expected bin|lib|example|test|bench"
            )),
        }
    }

    pub fn cargo_selector(self) -> &'static str {
        match self {
            Self::Bin => "--bin",
            Self::Lib => "--lib",
            Self::Example => "--example",
            Self::Test => "--test",
            Self::Bench => "--bench",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CargoCliConfig {
    pub mode: Option<AnalysisMode>,
    pub package: Option<String>,
    pub target_name: Option<String>,
    pub target_kind: Option<CargoTargetKind>,
    pub entry: Option<String>,
    pub api_roots: Vec<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target_triple: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencySummary {
    pub name: String,
    pub req: String,
    pub kind: Option<String>,
    pub optional: bool,
    pub uses_default_features: bool,
    pub features: Vec<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMemberSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoTargetPlan {
    pub name: String,
    pub kind: CargoTargetKind,
    pub src_path: String,
    pub crate_types: Vec<String>,
    pub required_features: Vec<String>,
    pub edition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoAnalysisPlan {
    pub schema_version: u32,
    pub metadata_format_version: u32,
    pub cargo_resolver_policy: String,
    pub analysis_mode: AnalysisMode,
    pub workspace_root: String,
    pub manifest_path: String,
    pub workspace_members: Vec<WorkspaceMemberSummary>,
    pub package_id: String,
    pub package_name: String,
    pub package_version: String,
    pub package_source: Option<String>,
    pub package_manifest_path: String,
    pub package_root: String,
    pub selected_target: CargoTargetPlan,
    pub analysis_roots: Vec<String>,
    pub requested_features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub resolved_package_features: Vec<String>,
    pub resolved_package_features_source: String,
    pub target_triple: Option<String>,
    pub cargo_version: String,
    pub rustc_version_verbose: String,
    pub rustup_toolchain_env: Option<String>,
    pub direct_dependencies: Vec<DependencySummary>,
}


/// Normalize a user-facing v6O semantic root into a local rustc DefPath suffix.
///
/// Scientific boundary:
/// - an unqualified local defining path (`foo::bar`) is preserved;
/// - `crate::foo::bar` is interpreted relative to the selected crate root;
/// - `<crate-name>::foo::bar` is accepted as the externally-spelled crate path
///   and its crate namespace is removed before matching local rustc DefPaths;
/// - no `use`/re-export alias resolution is attempted here.  A path that does
///   not denote the local defining DefPath after this namespace normalization
///   still fails closed.
///
/// Cargo documents that the library target name is the crate name used by
/// dependencies and that default library names replace `-` with `_`.  Accepting
/// both spellings here is therefore a syntactic convenience only; it does not
/// change target selection or semantic resolution.
pub fn normalize_local_def_path_request(requested: &str, local_crate_name: &str) -> String {
    let value = requested
        .strip_prefix("rust::")
        .unwrap_or(requested)
        .trim_end_matches("::bb0")
        .to_string();

    if let Some(rest) = value.strip_prefix("crate::") {
        return rest.to_string();
    }

    let crate_ident = local_crate_name.replace('-', "_");
    for prefix in [local_crate_name, crate_ident.as_str()] {
        if let Some(rest) = value.strip_prefix(prefix) {
            if let Some(rest) = rest.strip_prefix("::") {
                return rest.to_string();
            }
        }
    }

    value
}

impl CargoAnalysisPlan {
    pub fn write_json(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize Cargo analysis plan: {e}"))?;
        fs::write(path, json)
            .map_err(|e| format!("failed to write Cargo analysis plan {}: {e}", path.display()))
    }
}


fn command_stdout(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

const V6O_CARGO_RESOLVER_POLICY: &str = "metadata-no-deps+build-avoid-dev-deps";
const V6O_FEATURE_RESOLUTION_SOURCE: &str = "manifest_feature_closure_v1";

fn append_v6o_build_resolver_args(cmd: &mut Command) {
    // Cargo documents avoid-dev-deps for build/install-style operations. v6O
    // uses it only for the selected target build. Metadata discovery uses the
    // stable --no-deps interface instead, so unrelated dev-dependencies cannot
    // block package/target discovery by requiring a newer Cargo manifest syntax.
    cmd.arg("-Z").arg("avoid-dev-deps");
}

fn normalize_requested_feature(raw: &str, package_name: &str) -> Result<String, String> {
    if let Some((prefix, feature)) = raw.split_once('/') {
        if prefix != package_name {
            return Err(format!(
                "feature '{raw}' is qualified for package '{prefix}', but selected package is '{package_name}'"
            ));
        }
        if feature.is_empty() {
            return Err(format!("empty feature in '{raw}'"));
        }
        Ok(feature.to_string())
    } else if raw.is_empty() {
        Err("empty Cargo feature name".to_string())
    } else {
        Ok(raw.to_string())
    }
}

fn package_feature_definitions(package: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let raw = package
        .get("features")
        .and_then(Value::as_object)
        .ok_or_else(|| "cargo metadata v1 package missing features map".to_string())?;
    let mut defs = BTreeMap::new();
    for (name, values) in raw {
        let values = values
            .as_array()
            .ok_or_else(|| format!("feature '{name}' is not an array in cargo metadata"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| format!("feature '{name}' contains a non-string member"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        defs.insert(name.clone(), values);
    }
    Ok(defs)
}

fn resolve_root_package_feature_closure(
    package: &Value,
    cfg: &CargoCliConfig,
    package_name: &str,
) -> Result<Vec<String>, String> {
    let defs = package_feature_definitions(package)?;
    let mut enabled = BTreeSet::<String>::new();

    if cfg.all_features {
        enabled.extend(defs.keys().cloned());
    } else {
        if !cfg.no_default_features && defs.contains_key("default") {
            enabled.insert("default".to_string());
        }
        for raw in &cfg.features {
            let feature = normalize_requested_feature(raw, package_name)?;
            if !defs.contains_key(&feature) {
                return Err(format!(
                    "selected package {package_name} does not define requested feature '{feature}'"
                ));
            }
            enabled.insert(feature);
        }
    }

    loop {
        let before = enabled.len();
        let snapshot = enabled.iter().cloned().collect::<Vec<_>>();
        for feature in snapshot {
            let Some(edges) = defs.get(&feature) else { continue };
            for edge in edges {
                // dep:foo activates an optional dependency without adding a root
                // feature named foo. foo?/bar does not activate foo. For foo/bar,
                // Cargo activates foo when it exists as an implicit optional-dep
                // feature, represented as a key in the metadata feature map.
                if edge.starts_with("dep:") || edge.contains("?/") {
                    continue;
                }
                if let Some((left, _)) = edge.split_once('/') {
                    if defs.contains_key(left) {
                        enabled.insert(left.to_string());
                    }
                } else if defs.contains_key(edge) {
                    enabled.insert(edge.clone());
                }
            }
        }
        if enabled.len() == before {
            break;
        }
    }
    Ok(enabled.into_iter().collect())
}

fn append_feature_args(cmd: &mut Command, cfg: &CargoCliConfig) -> Result<(), String> {
    if cfg.all_features && !cfg.features.is_empty() {
        return Err("--all-features and --features are mutually exclusive in CREMA v6O-r1".to_string());
    }
    if cfg.all_features {
        cmd.arg("--all-features");
    }
    if cfg.no_default_features {
        cmd.arg("--no-default-features");
    }
    if !cfg.features.is_empty() {
        cmd.arg("--features").arg(cfg.features.join(","));
    }
    Ok(())
}

fn target_kind_from_json(target: &Value) -> Option<CargoTargetKind> {
    let kinds = target.get("kind")?.as_array()?;
    let has = |needle: &str| kinds.iter().any(|v| v.as_str() == Some(needle));
    if has("bin") {
        Some(CargoTargetKind::Bin)
    } else if has("example") {
        Some(CargoTargetKind::Example)
    } else if has("test") {
        Some(CargoTargetKind::Test)
    } else if has("bench") {
        Some(CargoTargetKind::Bench)
    } else if kinds.iter().any(|v| {
        matches!(
            v.as_str(),
            Some("lib")
                | Some("rlib")
                | Some("dylib")
                | Some("cdylib")
                | Some("staticlib")
        )
    }) {
        Some(CargoTargetKind::Lib)
    } else {
        None
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}


fn run_metadata_json(
    manifest_path: &Path,
    cfg: &CargoCliConfig,
    include_feature_selection: bool,
) -> Result<Value, String> {
    let mut cmd = Command::new("cargo");
    cmd.arg("metadata")
        .arg("--no-deps")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(manifest_path);
    if include_feature_selection {
        append_feature_args(&mut cmd, cfg)?;
    }
    let output = cmd
        .output()
        .map_err(|e| format!("failed to execute cargo metadata --format-version 1: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata --format-version 1 failed for {} (status={}): {}",
            manifest_path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("cargo metadata returned invalid JSON: {e}"))
}

pub fn discover_analysis_plan(project_path: &Path, cfg: &CargoCliConfig) -> Result<CargoAnalysisPlan, String> {
    let mode = cfg.mode.ok_or_else(|| "internal error: v6O discovery requested without --analysis-mode".to_string())?;
    let manifest_path = project_path.join("Cargo.toml");
    if !manifest_path.is_file() {
        return Err(format!("{} is not a Cargo manifest root", project_path.display()));
    }

    // Phase A: unconfigured metadata is used only to identify the selected
    // workspace package deterministically.  Cargo metadata has no `-p` option;
    // after package selection we re-run metadata with that member manifest so
    // unqualified --features apply to the requested member rather than to an
    // unrelated workspace root package.
    let base_metadata = run_metadata_json(&manifest_path, cfg, false)?;
    if base_metadata.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("cargo metadata returned an unsupported format version; v6O-r1 requires version=1".to_string());
    }
    let metadata = base_metadata.clone();

    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "cargo metadata v1 missing packages array".to_string())?;
    let workspace_ids: BTreeSet<String> = string_array(metadata.get("workspace_members"))
        .into_iter()
        .collect();
    if workspace_ids.is_empty() {
        return Err("cargo metadata v1 returned no workspace members".to_string());
    }

    if mode == AnalysisMode::Workspace && cfg.package.is_none() {
        return Err("--analysis-mode workspace requires --package/-p to select one workspace member".to_string());
    }

    let mut member_packages: Vec<&Value> = packages
        .iter()
        .filter(|p| {
            p.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| workspace_ids.contains(id))
        })
        .collect();
    member_packages.sort_by_key(|p| p.get("name").and_then(Value::as_str).unwrap_or(""));

    let mut workspace_members = Vec::new();
    for p in &member_packages {
        workspace_members.push(WorkspaceMemberSummary {
            id: p.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            name: p.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            version: p.get("version").and_then(Value::as_str).unwrap_or("").to_string(),
            manifest_path: p
                .get("manifest_path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }

    let selected_packages: Vec<&Value> = if let Some(name) = cfg.package.as_deref() {
        member_packages
            .iter()
            .copied()
            .filter(|p| p.get("name").and_then(Value::as_str) == Some(name))
            .collect()
    } else {
        member_packages.clone()
    };

    if selected_packages.len() != 1 {
        let choices = member_packages
            .iter()
            .map(|p| p.get("name").and_then(Value::as_str).unwrap_or("<unknown>"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "CREMA v6O requires exactly one workspace package per analysis; selected={} choices=[{}]. Use --package/-p.",
            selected_packages.len(), choices
        ));
    }
    let initially_selected = selected_packages[0];
    let selected_manifest = initially_selected
        .get("manifest_path")
        .and_then(Value::as_str)
        .ok_or("selected package missing manifest_path")?;

    let metadata = run_metadata_json(Path::new(selected_manifest), cfg, true)?;
    if metadata.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("configured cargo metadata returned an unsupported format version; v6O-r1 requires version=1".to_string());
    }
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "configured cargo metadata v1 missing packages array".to_string())?;
    let package = packages
        .iter()
        .find(|p| p.get("manifest_path").and_then(Value::as_str) == Some(selected_manifest))
        .ok_or_else(|| format!("configured metadata lost selected package manifest {selected_manifest}"))?;
    let package_id = package.get("id").and_then(Value::as_str).ok_or("selected package missing id")?;
    let package_name = package.get("name").and_then(Value::as_str).ok_or("selected package missing name")?;
    let package_version = package.get("version").and_then(Value::as_str).unwrap_or("");
    let package_manifest_path = package
        .get("manifest_path")
        .and_then(Value::as_str)
        .ok_or("selected package missing manifest_path")?;
    let package_root = Path::new(package_manifest_path)
        .parent()
        .ok_or_else(|| format!("package manifest has no parent: {package_manifest_path}"))?;

    let targets = package
        .get("targets")
        .and_then(Value::as_array)
        .ok_or("selected package missing targets")?;
    let mut candidates: Vec<(CargoTargetKind, &Value)> = targets
        .iter()
        .filter_map(|t| target_kind_from_json(t).map(|kind| (kind, t)))
        .collect();

    match mode {
        AnalysisMode::Application => {
            candidates.retain(|(kind, _)| *kind != CargoTargetKind::Lib);
            if cfg.target_kind.is_none() && candidates.iter().any(|(k, _)| *k == CargoTargetKind::Bin) {
                candidates.retain(|(kind, _)| *kind == CargoTargetKind::Bin);
            }
        }
        AnalysisMode::Library => {
            if cfg.target_kind.is_some_and(|k| k != CargoTargetKind::Lib) {
                return Err("--analysis-mode library is incompatible with non-lib --cargo-kind".to_string());
            }
            candidates.retain(|(kind, _)| *kind == CargoTargetKind::Lib);
        }
        AnalysisMode::Workspace => {}
    }
    if let Some(kind) = cfg.target_kind {
        candidates.retain(|(candidate, _)| *candidate == kind);
    }
    if let Some(name) = cfg.target_name.as_deref() {
        candidates.retain(|(_, t)| t.get("name").and_then(Value::as_str) == Some(name));
    }
    candidates.sort_by(|a, b| {
        let ak = a.0;
        let bk = b.0;
        let an = a.1.get("name").and_then(Value::as_str).unwrap_or("");
        let bn = b.1.get("name").and_then(Value::as_str).unwrap_or("");
        (ak, an).cmp(&(bk, bn))
    });

    if candidates.len() != 1 {
        let choices = candidates
            .iter()
            .map(|(kind, t)| {
                format!(
                    "{}:{:?}:{}",
                    package_name,
                    kind,
                    t.get("name").and_then(Value::as_str).unwrap_or("<unknown>")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "CREMA v6O target selection is ambiguous or empty; matches={} [{}]. Use --cargo-target and/or --cargo-kind.",
            candidates.len(), choices
        ));
    }
    let (kind, target) = candidates.remove(0);
    let target_name = target.get("name").and_then(Value::as_str).ok_or("target missing name")?;
    let src_path = target.get("src_path").and_then(Value::as_str).ok_or("target missing src_path")?;
    let selected_target = CargoTargetPlan {
        name: target_name.to_string(),
        kind,
        src_path: src_path.to_string(),
        crate_types: sorted_unique(string_array(target.get("crate_types"))),
        required_features: sorted_unique(string_array(target.get("required-features"))),
        edition: target.get("edition").and_then(Value::as_str).unwrap_or("2021").to_string(),
    };

    let resolved_package_features =
        resolve_root_package_feature_closure(package, cfg, package_name)?;
    let resolved_feature_set: BTreeSet<&str> = resolved_package_features.iter().map(String::as_str).collect();
    let missing_required = selected_target
        .required_features
        .iter()
        .filter(|feature| !resolved_feature_set.contains(feature.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_required.is_empty() {
        return Err(format!(
            "selected Cargo target {} requires disabled features: {}",
            selected_target.name,
            missing_required.join(",")
        ));
    }

    let analysis_roots = if kind == CargoTargetKind::Lib {
        if cfg.api_roots.is_empty() {
            return Err("library target analysis requires one or more explicit --api-root <rustc-def-path> values; CREMA v6O never invents a library main".to_string());
        }
        sorted_unique(cfg.api_roots.clone())
    } else {
        if !cfg.api_roots.is_empty() {
            return Err("--api-root is only valid for library targets".to_string());
        }
        if matches!(kind, CargoTargetKind::Test | CargoTargetKind::Bench) && cfg.entry.is_none() {
            return Err("test/bench targets require an explicit --entry; CREMA v6O does not treat the Cargo-generated harness main as a user API root".to_string());
        }
        vec![cfg.entry.clone().unwrap_or_else(|| "main".to_string())]
    };

    let mut direct_dependencies = package
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|d| DependencySummary {
            name: d.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            req: d.get("req").and_then(Value::as_str).unwrap_or("").to_string(),
            kind: d.get("kind").and_then(Value::as_str).map(ToOwned::to_owned),
            optional: d.get("optional").and_then(Value::as_bool).unwrap_or(false),
            uses_default_features: d
                .get("uses_default_features")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            features: sorted_unique(string_array(d.get("features"))),
            target: d.get("target").and_then(Value::as_str).map(ToOwned::to_owned),
        })
        .collect::<Vec<_>>();
    direct_dependencies.sort_by(|a, b| (&a.name, &a.req).cmp(&(&b.name, &b.req)));

    let workspace_root = metadata
        .get("workspace_root")
        .and_then(Value::as_str)
        .ok_or("cargo metadata v1 missing workspace_root")?;

    Ok(CargoAnalysisPlan {
        schema_version: 1,
        metadata_format_version: 1,
        cargo_resolver_policy: V6O_CARGO_RESOLVER_POLICY.to_string(),
        analysis_mode: mode,
        workspace_root: workspace_root.to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        workspace_members,
        package_id: package_id.to_string(),
        package_name: package_name.to_string(),
        package_version: package_version.to_string(),
        package_source: package.get("source").and_then(Value::as_str).map(ToOwned::to_owned),
        package_manifest_path: package_manifest_path.to_string(),
        package_root: package_root.to_string_lossy().to_string(),
        selected_target,
        analysis_roots,
        requested_features: sorted_unique(cfg.features.clone()),
        all_features: cfg.all_features,
        no_default_features: cfg.no_default_features,
        resolved_package_features,
        resolved_package_features_source: V6O_FEATURE_RESOLUTION_SOURCE.to_string(),
        target_triple: cfg.target_triple.clone(),
        cargo_version: command_stdout("cargo", &["--version"]),
        rustc_version_verbose: command_stdout("rustc", &["--version", "--verbose"]),
        rustup_toolchain_env: env::var("RUSTUP_TOOLCHAIN").ok(),
        direct_dependencies,
    })
}

pub struct CargoBuildInvocation<'a> {
    pub project_manifest: &'a Path,
    pub tool_executable: &'a Path,
    pub tool_dir: &'a Path,
    pub svf_output_dir: &'a Path,
    pub ffi_functions_path: &'a Path,
    pub icfg_output_path: &'a Path,
    pub semantic_root: &'a str,
    pub cargo_target_dir: &'a Path,
}

pub fn run_selected_target_with_cargo(
    plan: &CargoAnalysisPlan,
    cfg: &CargoCliConfig,
    invocation: CargoBuildInvocation<'_>,
) -> Result<(), String> {
    fs::create_dir_all(invocation.cargo_target_dir).map_err(|e| {
        format!(
            "failed to create isolated Cargo target dir {}: {e}",
            invocation.cargo_target_dir.display()
        )
    })?;
    if invocation.icfg_output_path.exists() {
        fs::remove_file(invocation.icfg_output_path).map_err(|e| {
            format!("failed to remove stale {}: {e}", invocation.icfg_output_path.display())
        })?;
    }

    let mut cmd = Command::new("cargo");
    match plan.selected_target.kind {
        CargoTargetKind::Test => {
            cmd.arg("test").arg("--no-run");
        }
        CargoTargetKind::Bench => {
            cmd.arg("bench").arg("--no-run");
        }
        _ => {
            cmd.arg("build");
        }
    }
    append_v6o_build_resolver_args(&mut cmd);
    cmd.arg("--manifest-path")
        .arg(invocation.project_manifest)
        .arg("--package")
        .arg(&plan.package_name);

    match plan.selected_target.kind {
        CargoTargetKind::Lib => {
            cmd.arg("--lib");
        }
        kind => {
            cmd.arg(kind.cargo_selector()).arg(&plan.selected_target.name);
        }
    }
    append_feature_args(&mut cmd, cfg)?;
    if let Some(triple) = plan.target_triple.as_deref() {
        cmd.arg("--target").arg(triple);
    }

    // Scientific boundary: Cargo owns the rustc invocation. CREMA observes the
    // exact workspace-member invocation through the documented workspace wrapper
    // interface instead of reconstructing --cfg/--extern/edition/target flags.
    cmd.env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", invocation.tool_executable)
        .env("CREMA_V6O_RUSTC_WRAPPER_MODE", "1")
        .env("CREMA_V6O_SELECTED_SRC", &plan.selected_target.src_path)
        .env("CREMA_V6O_SVF_OUTPUT_DIR", invocation.svf_output_dir)
        .env("CREMA_V6O_FFI_FUNCTIONS_PATH", invocation.ffi_functions_path)
        .env("CREMA_V6O_ICFG_OUTPUT_PATH", invocation.icfg_output_path)
        .env("CREMA_V6O_ENTRY_HINT", invocation.semantic_root)
        .env("CARGO_TARGET_DIR", invocation.cargo_target_dir)
        .env("CARGO_INCREMENTAL", "0")
        .current_dir(invocation.tool_dir);

    let status = cmd
        .status()
        .map_err(|e| format!("failed to execute Cargo target build under CREMA wrapper: {e}"))?;
    if !status.success() {
        return Err(format!(
            "Cargo target build failed under CREMA v6O wrapper with status {status}"
        ));
    }
    if !invocation.icfg_output_path.is_file() {
        return Err(format!(
            "Cargo build succeeded but selected target was not observed by CREMA wrapper; missing {}",
            invocation.icfg_output_path.display()
        ));
    }
    Ok(())
}

pub fn isolated_cargo_target_dir(tool_dir: &Path, root: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let slug: String = root
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    tool_dir
        .join("target")
        .join("crema-v6o-cargo")
        .join(format!("{}-{}-{nonce}", std::process::id(), slug))
}

pub fn wrapper_selected_source_matches(args: &[String], selected_src: &Path) -> bool {
    let selected = fs::canonicalize(selected_src).unwrap_or_else(|_| selected_src.to_path_buf());
    args.iter().any(|arg| {
        if !arg.ends_with(".rs") {
            return false;
        }
        let p = PathBuf::from(arg);
        let candidate = if p.is_absolute() {
            p
        } else {
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(p)
        };
        fs::canonicalize(&candidate).unwrap_or(candidate) == selected
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_kind_parser_is_fail_closed() {
        assert_eq!(CargoTargetKind::parse("bin").unwrap(), CargoTargetKind::Bin);
        assert_eq!(CargoTargetKind::parse("lib").unwrap(), CargoTargetKind::Lib);
        assert!(CargoTargetKind::parse("proc-magic").is_err());
    }

    #[test]
    fn analysis_mode_parser_is_fail_closed() {
        assert_eq!(AnalysisMode::parse("application").unwrap(), AnalysisMode::Application);
        assert_eq!(AnalysisMode::parse("library").unwrap(), AnalysisMode::Library);
        assert_eq!(AnalysisMode::parse("workspace").unwrap(), AnalysisMode::Workspace);
        assert!(AnalysisMode::parse("auto").is_err());
    }

    #[test]
    fn feature_selection_rejects_all_plus_named() {
        let cfg = CargoCliConfig {
            all_features: true,
            features: vec!["serde".into()],
            ..CargoCliConfig::default()
        };
        let mut cmd = Command::new("cargo");
        assert!(append_feature_args(&mut cmd, &cfg).is_err());
    }

    #[test]
    fn build_resolver_policy_uses_avoid_dev_deps_only_for_build() {
        let mut cmd = Command::new("cargo");
        cmd.arg("build");
        append_v6o_build_resolver_args(&mut cmd);
        let args = cmd
            .get_args()
            .map(|x| x.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["build", "-Z", "avoid-dev-deps"]);
    }

    fn feature_package(features: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"features": features})
    }

    #[test]
    fn manifest_feature_closure_honors_default_and_nested_features() {
        let package = feature_package(serde_json::json!({
            "default": ["default-path"],
            "default-path": ["inner"],
            "inner": [],
            "extra": []
        }));
        let cfg = CargoCliConfig::default();
        assert_eq!(
            resolve_root_package_feature_closure(&package, &cfg, "p").unwrap(),
            vec!["default", "default-path", "inner"]
        );
    }

    #[test]
    fn manifest_feature_closure_honors_no_default_and_package_qualified_request() {
        let package = feature_package(serde_json::json!({
            "default": ["default-path"],
            "default-path": [],
            "extra": []
        }));
        let cfg = CargoCliConfig {
            no_default_features: true,
            features: vec!["p/extra".to_string()],
            ..CargoCliConfig::default()
        };
        assert_eq!(
            resolve_root_package_feature_closure(&package, &cfg, "p").unwrap(),
            vec!["extra"]
        );
    }

    #[test]
    fn manifest_feature_closure_handles_dependency_forwarding_conservatively() {
        let package = feature_package(serde_json::json!({
            "default": ["serde-support"],
            "serde-support": ["dep:serde", "serde/std", "log?/std"],
            "serde": []
        }));
        let cfg = CargoCliConfig::default();
        assert_eq!(
            resolve_root_package_feature_closure(&package, &cfg, "p").unwrap(),
            vec!["default", "serde", "serde-support"]
        );
    }

    #[test]
    fn local_root_namespace_normalizes_crate_qualified_paths() {
        assert_eq!(
            normalize_local_def_path_request("unicode_ident::is_xid_start", "unicode_ident"),
            "is_xid_start"
        );
        assert_eq!(
            normalize_local_def_path_request("ryu::pretty::format32", "ryu"),
            "pretty::format32"
        );
        assert_eq!(
            normalize_local_def_path_request("memchr::memchr::memchr", "memchr"),
            "memchr::memchr"
        );
    }

    #[test]
    fn local_root_namespace_normalizes_crate_keyword_and_rust_node_syntax() {
        assert_eq!(
            normalize_local_def_path_request("crate::api::root", "example"),
            "api::root"
        );
        assert_eq!(
            normalize_local_def_path_request("rust::example::api::root::bb0", "example"),
            "api::root"
        );
    }

    #[test]
    fn local_root_namespace_does_not_strip_unrelated_module_prefix() {
        assert_eq!(
            normalize_local_def_path_request("other::api::root", "example"),
            "other::api::root"
        );
        assert_eq!(
            normalize_local_def_path_request("my_crate::api", "my-crate"),
            "api"
        );
    }

}
