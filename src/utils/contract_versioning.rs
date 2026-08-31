//! Contract version management.
//!
//! Tracks the semantic version history of a contract (`contract-versions.toml`),
//! detects version conflicts across the dependency graph declared via
//! `contract-dependencies.toml` (see [`crate::utils::contract_deps`]), builds a
//! version compatibility matrix, and resolves migration paths between versions
//! using the migration rule files produced by `starforge migrate init`.

use crate::utils::contract_deps::{self, DependencySource};
use anyhow::{Context, Result};
use chrono::Utc;
use semver::{Comparator, Op, Prerelease, Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_FILE: &str = "contract-versions.toml";

// ── Version tracking ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionRecord {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_hash: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub yanked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VersionManifest {
    #[serde(default)]
    pub contract: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(default)]
    pub versions: Vec<VersionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpPart {
    Major,
    Minor,
    Patch,
    Prerelease,
}

impl std::str::FromStr for BumpPart {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "major" => Ok(BumpPart::Major),
            "minor" => Ok(BumpPart::Minor),
            "patch" => Ok(BumpPart::Patch),
            "prerelease" | "pre" => Ok(BumpPart::Prerelease),
            other => anyhow::bail!(
                "Unknown bump part '{}'; expected major, minor, patch, or prerelease",
                other
            ),
        }
    }
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join(MANIFEST_FILE)
}

pub fn init(dir: &Path, contract: &str) -> Result<VersionManifest> {
    let path = manifest_path(dir);
    if path.exists() {
        anyhow::bail!("{} already exists in this directory", MANIFEST_FILE);
    }
    let manifest = VersionManifest {
        contract: contract.to_string(),
        current: None,
        versions: Vec::new(),
    };
    save(&path, &manifest)?;
    Ok(manifest)
}

pub fn load(path: &Path) -> Result<VersionManifest> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest =
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(manifest)
}

pub fn save(path: &Path, manifest: &VersionManifest) -> Result<()> {
    let content = toml::to_string_pretty(manifest)?;
    fs::write(path, content)?;
    Ok(())
}

fn highest_version(manifest: &VersionManifest) -> Option<Version> {
    manifest
        .versions
        .iter()
        .filter_map(|v| Version::parse(&v.version).ok())
        .max()
}

/// Record an explicit version. Fails if it does not exceed the current
/// highest tracked version unless `force` is set.
pub fn tag(
    dir: &Path,
    version_str: &str,
    notes: Option<String>,
    wasm_hash: Option<String>,
    force: bool,
) -> Result<VersionRecord> {
    let path = manifest_path(dir);
    let mut manifest = if path.exists() {
        load(&path)?
    } else {
        VersionManifest::default()
    };

    let version =
        Version::parse(version_str).context("Version must be valid semantic version (x.y.z)")?;

    if let Some(highest) = highest_version(&manifest) {
        if version <= highest && !force {
            anyhow::bail!(
                "Version {} is not greater than current highest version {} (use --force to override)",
                version,
                highest
            );
        }
    }

    let record = VersionRecord {
        version: version.to_string(),
        notes,
        wasm_hash,
        created_at: Utc::now().to_rfc3339(),
        yanked: false,
    };

    manifest.versions.push(record.clone());
    manifest.current = Some(record.version.clone());
    save(&path, &manifest)?;
    Ok(record)
}

/// Bump the current tracked version by the given semantic versioning part.
pub fn bump(dir: &Path, part: BumpPart, notes: Option<String>) -> Result<VersionRecord> {
    let path = manifest_path(dir);
    let manifest = if path.exists() {
        load(&path)?
    } else {
        VersionManifest::default()
    };

    let mut next = match highest_version(&manifest) {
        Some(v) => v,
        None => Version::new(0, 0, 0),
    };

    match part {
        BumpPart::Major => {
            next.major += 1;
            next.minor = 0;
            next.patch = 0;
            next.pre = Prerelease::EMPTY;
        }
        BumpPart::Minor => {
            next.minor += 1;
            next.patch = 0;
            next.pre = Prerelease::EMPTY;
        }
        BumpPart::Patch => {
            next.patch += 1;
            next.pre = Prerelease::EMPTY;
        }
        BumpPart::Prerelease => {
            let n: u64 = next
                .pre
                .as_str()
                .rsplit('.')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            next.pre = Prerelease::new(&format!("rc.{}", n + 1))
                .context("Failed to construct prerelease identifier")?;
        }
    }

    tag(dir, &next.to_string(), notes, None, false)
}

pub fn list(dir: &Path) -> Result<Vec<VersionRecord>> {
    let path = manifest_path(dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(load(&path)?.versions)
}

pub fn find(dir: &Path, version_str: &str) -> Result<Option<VersionRecord>> {
    let path = manifest_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let manifest = load(&path)?;
    Ok(manifest
        .versions
        .into_iter()
        .find(|v| v.version == version_str))
}

pub fn yank(dir: &Path, version_str: &str) -> Result<()> {
    let path = manifest_path(dir);
    let mut manifest = load(&path)?;
    let record = manifest
        .versions
        .iter_mut()
        .find(|v| v.version == version_str)
        .with_context(|| format!("Version '{}' not tracked", version_str))?;
    record.yanked = true;
    save(&path, &manifest)?;
    Ok(())
}

// ── Version interval math (for conflict detection) ──────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Bound {
    value: Version,
    inclusive: bool,
}

#[derive(Debug, Clone)]
struct Interval {
    lower: Option<Bound>,
    upper: Option<Bound>,
}

impl Interval {
    fn unbounded() -> Self {
        Interval {
            lower: None,
            upper: None,
        }
    }

    fn is_empty(&self) -> bool {
        match (&self.lower, &self.upper) {
            (Some(lo), Some(hi)) => {
                if lo.value > hi.value {
                    true
                } else if lo.value == hi.value {
                    !(lo.inclusive && hi.inclusive)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn intersect(&self, other: &Interval) -> Interval {
        let lower = tighter_lower(&self.lower, &other.lower);
        let upper = tighter_upper(&self.upper, &other.upper);
        Interval { lower, upper }
    }
}

fn tighter_lower(a: &Option<Bound>, b: &Option<Bound>) -> Option<Bound> {
    match (a, b) {
        (None, None) => None,
        (Some(x), None) => Some(x.clone()),
        (None, Some(y)) => Some(y.clone()),
        (Some(x), Some(y)) => {
            if x.value > y.value {
                Some(x.clone())
            } else if y.value > x.value {
                Some(y.clone())
            } else {
                Some(Bound {
                    value: x.value.clone(),
                    inclusive: x.inclusive && y.inclusive,
                })
            }
        }
    }
}

fn tighter_upper(a: &Option<Bound>, b: &Option<Bound>) -> Option<Bound> {
    match (a, b) {
        (None, None) => None,
        (Some(x), None) => Some(x.clone()),
        (None, Some(y)) => Some(y.clone()),
        (Some(x), Some(y)) => {
            if x.value < y.value {
                Some(x.clone())
            } else if y.value < x.value {
                Some(y.clone())
            } else {
                Some(Bound {
                    value: x.value.clone(),
                    inclusive: x.inclusive && y.inclusive,
                })
            }
        }
    }
}

fn base_version(major: u64, minor: Option<u64>, patch: Option<u64>) -> Version {
    Version::new(major, minor.unwrap_or(0), patch.unwrap_or(0))
}

/// Smallest version strictly greater than every version matching the given
/// major/minor/patch prefix (i.e. the exclusive upper bound of that prefix).
fn next_after_prefix(major: u64, minor: Option<u64>, patch: Option<u64>) -> Version {
    match (minor, patch) {
        (Some(m), Some(p)) => Version::new(major, m, p + 1),
        (Some(m), None) => Version::new(major, m + 1, 0),
        (None, _) => Version::new(major + 1, 0, 0),
    }
}

fn caret_upper(major: u64, minor: Option<u64>, patch: Option<u64>) -> Version {
    if major > 0 {
        Version::new(major + 1, 0, 0)
    } else if let Some(m) = minor {
        if m > 0 {
            Version::new(0, m + 1, 0)
        } else if patch.is_some() {
            Version::new(0, 0, patch.unwrap() + 1)
        } else {
            Version::new(0, 1, 0)
        }
    } else {
        Version::new(1, 0, 0)
    }
}

fn comparator_interval(c: &Comparator) -> Interval {
    let v = base_version(c.major, c.minor, c.patch);
    match c.op {
        Op::Exact => Interval {
            lower: Some(Bound {
                value: v.clone(),
                inclusive: true,
            }),
            upper: Some(Bound {
                value: next_after_prefix(c.major, c.minor, c.patch),
                inclusive: false,
            }),
        },
        Op::Greater => Interval {
            lower: Some(Bound {
                value: v,
                inclusive: false,
            }),
            upper: None,
        },
        Op::GreaterEq => Interval {
            lower: Some(Bound {
                value: v,
                inclusive: true,
            }),
            upper: None,
        },
        Op::Less => Interval {
            lower: None,
            upper: Some(Bound {
                value: v,
                inclusive: false,
            }),
        },
        Op::LessEq => Interval {
            lower: None,
            upper: Some(Bound {
                value: v,
                inclusive: true,
            }),
        },
        Op::Tilde => Interval {
            lower: Some(Bound {
                value: v.clone(),
                inclusive: true,
            }),
            upper: Some(Bound {
                value: if c.minor.is_some() {
                    Version::new(c.major, c.minor.unwrap() + 1, 0)
                } else {
                    Version::new(c.major + 1, 0, 0)
                },
                inclusive: false,
            }),
        },
        Op::Caret => Interval {
            lower: Some(Bound {
                value: v,
                inclusive: true,
            }),
            upper: Some(Bound {
                value: caret_upper(c.major, c.minor, c.patch),
                inclusive: false,
            }),
        },
        Op::Wildcard => Interval {
            lower: Some(Bound {
                value: v,
                inclusive: true,
            }),
            upper: Some(Bound {
                value: next_after_prefix(c.major, c.minor, c.patch),
                inclusive: false,
            }),
        },
        // semver::Op is #[non_exhaustive]; treat unknown ops as unbounded
        // rather than silently under-approximating a conflict.
        _ => Interval::unbounded(),
    }
}

/// Convert a `VersionReq` (an AND of comparators) into its combined interval.
fn req_interval(req: &VersionReq) -> Interval {
    req.comparators
        .iter()
        .map(comparator_interval)
        .fold(Interval::unbounded(), |acc, i| acc.intersect(&i))
}

// ── Dependency requirement collection ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RequirementEdge {
    /// Name of the contract declaring the dependency.
    pub requirer: String,
    /// Name of the depended-upon contract.
    pub dependency: String,
    /// Raw version constraint string as declared.
    pub raw: String,
    /// Resolved local path to the dependency, if declared (used to look up
    /// its own tracked version history).
    pub dep_path: Option<PathBuf>,
}

/// Walk the dependency graph rooted at `dir` (same traversal contract_deps
/// uses for resolution) and collect every version requirement edge.
pub fn collect_requirements(dir: &Path) -> Result<Vec<RequirementEdge>> {
    let mut edges = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = vec![(dir.to_path_buf(), "root".to_string())];

    while let Some((curr_dir, node_name)) = queue.pop() {
        if visited.contains(&node_name) {
            continue;
        }
        visited.insert(node_name.clone());

        let path = curr_dir.join("contract-dependencies.toml");
        if !path.exists() {
            continue;
        }
        let deps = match contract_deps::load(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        for (dep_name, source) in deps.dependencies {
            let (raw, dep_path) = match &source {
                DependencySource::Version(v) => (Some(v.clone()), None),
                DependencySource::Detailed { version, path, .. } => {
                    (version.clone(), path.clone().map(|p| curr_dir.join(p)))
                }
            };

            if let Some(raw) = raw {
                edges.push(RequirementEdge {
                    requirer: node_name.clone(),
                    dependency: dep_name.clone(),
                    raw,
                    dep_path: dep_path.clone(),
                });
            }

            if let Some(dep_dir) = dep_path {
                queue.push((dep_dir, dep_name));
            }
        }
    }

    Ok(edges)
}

// ── Conflict detection ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Conflict {
    pub dependency: String,
    /// (requirer, raw constraint) pairs contributing to this dependency.
    pub requirers: Vec<(String, String)>,
    /// True if the constraints are structurally disjoint (no version could
    /// ever satisfy all of them simultaneously).
    pub structurally_impossible: bool,
    /// Versions from the dependency's own tracked history that satisfy every
    /// requirer's constraint, if that history is available.
    pub resolvable_versions: Vec<String>,
    /// Whether the dependency's own version history was available to check.
    pub history_checked: bool,
}

/// Detect version conflicts across every dependency name referenced more
/// than once in the graph rooted at `dir`.
pub fn detect_conflicts(dir: &Path) -> Result<Vec<Conflict>> {
    let edges = collect_requirements(dir)?;
    let mut by_dep: HashMap<String, Vec<&RequirementEdge>> = HashMap::new();
    for edge in &edges {
        by_dep
            .entry(edge.dependency.clone())
            .or_default()
            .push(edge);
    }

    let mut conflicts = Vec::new();
    for (dep_name, group) in by_dep {
        let distinct_raw: HashSet<&str> = group.iter().map(|e| e.raw.as_str()).collect();
        if distinct_raw.len() <= 1 {
            continue;
        }

        let reqs: Vec<(String, VersionReq)> = group
            .iter()
            .filter_map(|e| VersionReq::parse(&e.raw).ok().map(|r| (e.raw.clone(), r)))
            .collect();
        if reqs.is_empty() {
            continue;
        }

        let combined = reqs
            .iter()
            .map(|(_, r)| req_interval(r))
            .fold(Interval::unbounded(), |acc, i| acc.intersect(&i));
        let structurally_impossible = combined.is_empty();

        let dep_path = group.iter().find_map(|e| e.dep_path.clone());
        let (resolvable_versions, history_checked) = match dep_path {
            Some(p) if !structurally_impossible => match list(&p) {
                Ok(versions) if !versions.is_empty() => {
                    let matches: Vec<String> = versions
                        .iter()
                        .filter(|v| !v.yanked)
                        .filter_map(|v| Version::parse(&v.version).ok().map(|pv| (pv, v)))
                        .filter(|(pv, _)| reqs.iter().all(|(_, r)| r.matches(pv)))
                        .map(|(_, v)| v.version.clone())
                        .collect();
                    (matches, true)
                }
                _ => (Vec::new(), false),
            },
            _ => (Vec::new(), false),
        };

        let has_conflict =
            structurally_impossible || (history_checked && resolvable_versions.is_empty());

        if has_conflict {
            conflicts.push(Conflict {
                dependency: dep_name,
                requirers: group
                    .iter()
                    .map(|e| (e.requirer.clone(), e.raw.clone()))
                    .collect(),
                structurally_impossible,
                resolvable_versions,
                history_checked,
            });
        }
    }

    conflicts.sort_by(|a, b| a.dependency.cmp(&b.dependency));
    Ok(conflicts)
}

// ── Compatibility matrix ─────────────────────────────────────────────────────

pub struct CompatibilityMatrix {
    pub dependency: String,
    pub requirers: Vec<String>,
    pub versions: Vec<String>,
    /// cells[version_idx][requirer_idx] = compatible
    pub cells: Vec<Vec<bool>>,
}

/// Build a compatibility matrix for `dependency`: rows are its tracked
/// versions, columns are every requirer that declares a constraint on it.
pub fn build_matrix(dir: &Path, dependency: &str) -> Result<CompatibilityMatrix> {
    let edges = collect_requirements(dir)?;
    let relevant: Vec<&RequirementEdge> = edges
        .iter()
        .filter(|e| e.dependency == dependency)
        .collect();
    if relevant.is_empty() {
        anyhow::bail!("No requirer declares a dependency on '{}'", dependency);
    }

    let dep_path = relevant
        .iter()
        .find_map(|e| e.dep_path.clone())
        .with_context(|| {
            format!(
                "Cannot build a compatibility matrix for '{}': no local path resolved, so its version history is unknown",
                dependency
            )
        })?;
    let versions = list(&dep_path)?;
    if versions.is_empty() {
        anyhow::bail!(
            "'{}' has no tracked versions ({} not found or empty)",
            dependency,
            MANIFEST_FILE
        );
    }

    let requirers: Vec<String> = relevant.iter().map(|e| e.requirer.clone()).collect();
    let reqs: Vec<VersionReq> = relevant
        .iter()
        .map(|e| VersionReq::parse(&e.raw))
        .collect::<std::result::Result<_, _>>()
        .context("Invalid version requirement in dependency graph")?;

    let mut cells = Vec::with_capacity(versions.len());
    for v in &versions {
        let parsed = Version::parse(&v.version)?;
        let row: Vec<bool> = reqs.iter().map(|r| r.matches(&parsed)).collect();
        cells.push(row);
    }

    Ok(CompatibilityMatrix {
        dependency: dependency.to_string(),
        requirers,
        versions: versions.into_iter().map(|v| v.version).collect(),
        cells,
    })
}

// ── Migration path resolution ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MigrationEdge {
    pub from: String,
    pub to: String,
    pub file: PathBuf,
}

#[derive(Debug, Deserialize)]
struct MigrationFileMeta {
    from_version: String,
    to_version: String,
}

/// Scan `dir` for migration rule files (as produced by `starforge migrate
/// init`) and return each as a from -> to edge.
pub fn discover_migrations(dir: &Path) -> Result<Vec<MigrationEdge>> {
    let mut edges = Vec::new();
    if !dir.exists() {
        return Ok(edges);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if let Ok(meta) = toml::from_str::<MigrationFileMeta>(&content) {
            edges.push(MigrationEdge {
                from: meta.from_version,
                to: meta.to_version,
                file: path,
            });
        }
    }
    Ok(edges)
}

/// Find the shortest chain of migrations that moves storage from `from` to
/// `to`, potentially hopping through several intermediate versions.
pub fn find_migration_path(
    edges: &[MigrationEdge],
    from: &str,
    to: &str,
) -> Result<Vec<MigrationEdge>> {
    if from == to {
        return Ok(Vec::new());
    }

    let mut adjacency: HashMap<&str, Vec<&MigrationEdge>> = HashMap::new();
    for edge in edges {
        adjacency.entry(edge.from.as_str()).or_default().push(edge);
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    let mut came_from: HashMap<&str, &MigrationEdge> = HashMap::new();

    visited.insert(from);
    queue.push_back(from);

    while let Some(node) = queue.pop_front() {
        if node == to {
            break;
        }
        if let Some(next_edges) = adjacency.get(node) {
            for edge in next_edges {
                let next = edge.to.as_str();
                if visited.insert(next) {
                    came_from.insert(next, edge);
                    queue.push_back(next);
                }
            }
        }
    }

    if !visited.contains(to) {
        anyhow::bail!("No migration path found from '{}' to '{}'", from, to);
    }

    let mut path = Vec::new();
    let mut cursor = to;
    while cursor != from {
        let edge = came_from
            .get(cursor)
            .expect("came_from populated for every visited non-root node");
        path.push((*edge).clone());
        cursor = edge.from.as_str();
    }
    path.reverse();
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_init_tag_bump() {
        let dir = tempdir().unwrap();
        init(dir.path(), "token").unwrap();

        let r1 = tag(
            dir.path(),
            "1.0.0",
            Some("first release".into()),
            None,
            false,
        )
        .unwrap();
        assert_eq!(r1.version, "1.0.0");

        // Tagging a lower/equal version without --force should fail.
        assert!(tag(dir.path(), "0.9.0", None, None, false).is_err());
        assert!(tag(dir.path(), "0.9.0", None, None, true).is_ok());

        let bumped = bump(dir.path(), BumpPart::Minor, None).unwrap();
        assert_eq!(bumped.version, "1.1.0");

        let bumped_patch = bump(dir.path(), BumpPart::Patch, None).unwrap();
        assert_eq!(bumped_patch.version, "1.1.1");

        let versions = list(dir.path()).unwrap();
        assert_eq!(versions.len(), 4);
    }

    #[test]
    fn test_yank() {
        let dir = tempdir().unwrap();
        init(dir.path(), "token").unwrap();
        tag(dir.path(), "1.0.0", None, None, false).unwrap();
        yank(dir.path(), "1.0.0").unwrap();
        let record = find(dir.path(), "1.0.0").unwrap().unwrap();
        assert!(record.yanked);
    }

    #[test]
    fn test_interval_intersection_overlapping() {
        let a = req_interval(&VersionReq::parse("^1.2.0").unwrap());
        let b = req_interval(&VersionReq::parse(">=1.3.0, <1.9.0").unwrap());
        assert!(!a.intersect(&b).is_empty());
    }

    #[test]
    fn test_interval_intersection_disjoint() {
        let a = req_interval(&VersionReq::parse("^1.0.0").unwrap()); // [1.0.0, 2.0.0)
        let b = req_interval(&VersionReq::parse("^2.0.0").unwrap()); // [2.0.0, 3.0.0)
        assert!(a.intersect(&b).is_empty());
    }

    #[test]
    fn test_tilde_and_caret_bounds() {
        let tilde = req_interval(&VersionReq::parse("~1.2.3").unwrap());
        assert!(!tilde.is_empty());
        assert!(VersionReq::parse("~1.2.3")
            .unwrap()
            .matches(&Version::parse("1.2.9").unwrap()));
        assert!(!VersionReq::parse("~1.2.3")
            .unwrap()
            .matches(&Version::parse("1.3.0").unwrap()));

        let caret = req_interval(&VersionReq::parse("^0.2.3").unwrap());
        // ^0.2.3 := >=0.2.3, <0.3.0
        assert_eq!(caret.upper.as_ref().unwrap().value, Version::new(0, 3, 0));
    }

    fn write_deps(dir: &Path, contents: &str) {
        fs::write(dir.join("contract-dependencies.toml"), contents).unwrap();
    }

    /// Render a path for embedding in a TOML basic string.
    ///
    /// Windows paths contain backslashes, which TOML reads as escape
    /// sequences; forward slashes are accepted as separators on every platform.
    fn toml_path(path: &Path) -> String {
        path.display().to_string().replace('\\', "/")
    }

    #[test]
    fn test_detect_conflicts_disjoint_requirements() {
        let root = tempdir().unwrap();
        let dep = tempdir().unwrap();

        write_deps(
            root.path(),
            &format!(
                "[dependencies]\na = {{ version = \"^1.0.0\", path = \"{}\" }}\n",
                toml_path(dep.path())
            ),
        );

        // Simulate a second requirer with an incompatible constraint by
        // nesting another manifest under a sibling directory that also
        // requires `a`, pointing at the same dep path with a disjoint range.
        let sibling = tempdir().unwrap();
        write_deps(
            sibling.path(),
            &format!(
                "[dependencies]\na = {{ version = \"^2.0.0\", path = \"{}\" }}\n",
                toml_path(dep.path())
            ),
        );
        write_deps(
            root.path(),
            &format!(
                "[dependencies]\na = {{ version = \"^1.0.0\", path = \"{}\" }}\nb = {{ path = \"{}\" }}\n",
                toml_path(dep.path()),
                toml_path(sibling.path())
            ),
        );

        let conflicts = detect_conflicts(root.path()).unwrap();
        let a_conflict = conflicts.iter().find(|c| c.dependency == "a");
        assert!(a_conflict.is_some());
        assert!(a_conflict.unwrap().structurally_impossible);
    }

    #[test]
    fn test_detect_conflicts_none_when_compatible() {
        let root = tempdir().unwrap();
        let dep = tempdir().unwrap();
        write_deps(
            root.path(),
            &format!(
                "[dependencies]\na = {{ version = \"^1.0.0\", path = \"{}\" }}\n",
                toml_path(dep.path())
            ),
        );
        let conflicts = detect_conflicts(root.path()).unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_build_matrix() {
        let root = tempdir().unwrap();
        let dep = tempdir().unwrap();
        init(dep.path(), "token").unwrap();
        tag(dep.path(), "1.0.0", None, None, false).unwrap();
        tag(dep.path(), "1.5.0", None, None, false).unwrap();
        tag(dep.path(), "2.0.0", None, None, false).unwrap();

        write_deps(
            root.path(),
            &format!(
                "[dependencies]\ntoken = {{ version = \"^1.0.0\", path = \"{}\" }}\n",
                toml_path(dep.path())
            ),
        );

        let matrix = build_matrix(root.path(), "token").unwrap();
        assert_eq!(matrix.versions, vec!["1.0.0", "1.5.0", "2.0.0"]);
        assert_eq!(matrix.cells, vec![vec![true], vec![true], vec![false]]);
    }

    fn write_migration(dir: &Path, name: &str, from: &str, to: &str) {
        fs::write(
            dir.join(name),
            format!("from_version = \"{}\"\nto_version = \"{}\"\n", from, to),
        )
        .unwrap();
    }

    #[test]
    fn test_migration_path_direct_and_multi_hop() {
        let dir = tempdir().unwrap();
        write_migration(dir.path(), "v1_to_v2.toml", "v1", "v2");
        write_migration(dir.path(), "v2_to_v3.toml", "v2", "v3");
        write_migration(dir.path(), "v1_to_v3_shortcut.toml", "v1", "v3");

        let edges = discover_migrations(dir.path()).unwrap();
        assert_eq!(edges.len(), 3);

        // BFS should prefer the direct one-hop shortcut over the two-hop path.
        let path = find_migration_path(&edges, "v1", "v3").unwrap();
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].from, "v1");
        assert_eq!(path[0].to, "v3");
    }

    #[test]
    fn test_migration_path_requires_hop() {
        let dir = tempdir().unwrap();
        write_migration(dir.path(), "v1_to_v2.toml", "v1", "v2");
        write_migration(dir.path(), "v2_to_v3.toml", "v2", "v3");
        let edges = discover_migrations(dir.path()).unwrap();

        let path = find_migration_path(&edges, "v1", "v3").unwrap();
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].to, "v2");
        assert_eq!(path[1].to, "v3");
    }

    #[test]
    fn test_migration_path_missing() {
        let dir = tempdir().unwrap();
        write_migration(dir.path(), "v1_to_v2.toml", "v1", "v2");
        let edges = discover_migrations(dir.path()).unwrap();
        assert!(find_migration_path(&edges, "v1", "v9").is_err());
    }

    #[test]
    fn test_migration_path_same_version() {
        let edges: Vec<MigrationEdge> = Vec::new();
        assert!(find_migration_path(&edges, "v1", "v1").unwrap().is_empty());
    }
}
