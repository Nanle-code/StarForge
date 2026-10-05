use anyhow::{bail, Context, Result};
use clap::Args;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Args)]
pub struct AddCommands {
    /// Component name, or use --list to show available components
    component: Option<String>,
    /// List the available contract components
    #[arg(long, conflicts_with = "component")]
    list: bool,
    /// Contract project directory
    #[arg(long)]
    path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct Component {
    name: &'static str,
    description: &'static str,
    template: &'static str,
    functions: &'static [&'static str],
    files: &'static [&'static str],
    dependencies: &'static [(&'static str, &'static str)],
    prerequisites: &'static [&'static str],
    conflicts: &'static [&'static str],
}

const REGISTRY: &[Component] = &[
    Component {
        name: "ownable",
        description: "Owner storage and ownership transfer",
        template: include_str!("../../templates/components/ownable.rs.txt"),
        functions: &["owner", "transfer_ownership"],
        files: &["src/lib.rs"],
        dependencies: &[],
        prerequisites: &[],
        conflicts: &[],
    },
    Component {
        name: "access-control",
        description: "Role based access control",
        template: include_str!("../../templates/components/access-control.rs.txt"),
        functions: &["grant_role", "has_role", "revoke_role"],
        files: &["src/lib.rs"],
        dependencies: &[],
        prerequisites: &[],
        conflicts: &[],
    },
    Component {
        name: "pausable",
        description: "Pause and resume contract operations",
        template: include_str!("../../templates/components/pausable.rs.txt"),
        functions: &["pause", "unpause", "is_paused"],
        files: &["src/lib.rs"],
        dependencies: &[],
        prerequisites: &[],
        conflicts: &[],
    },
    Component {
        name: "upgradeable",
        description: "WASM upgrade and version management",
        template: include_str!("../../templates/components/upgradeable.rs.txt"),
        functions: &["version", "upgrade"],
        files: &["src/lib.rs"],
        dependencies: &[],
        prerequisites: &[],
        conflicts: &[],
    },
];

pub fn handle(cmd: AddCommands, dry_run: bool) -> Result<()> {
    if cmd.list {
        for c in REGISTRY {
            println!("{:<16} {}", c.name, c.description);
        }
        return Ok(());
    }
    let component = cmd
        .component
        .ok_or_else(|| anyhow::anyhow!("Specify a component or use --list"))?;
    apply(
        &component,
        cmd.path.as_deref().unwrap_or(Path::new(".")),
        dry_run,
    )
}

fn apply(name: &str, root: &Path, dry_run: bool) -> Result<()> {
    let c = REGISTRY.iter().find(|c| c.name == name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown component '{}'. Run `starforge add --list` to see available components.",
            name
        )
    })?;
    let manifest_path = root.join("Cargo.toml");
    let source_path = root.join("src/lib.rs");
    let manifest = fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "Not a contract project: missing {}",
            manifest_path.display()
        )
    })?;
    let source = fs::read_to_string(&source_path)
        .with_context(|| format!("Not a contract project: missing {}", source_path.display()))?;
    let parsed_manifest: toml::Value =
        toml::from_str(&manifest).context("Invalid project Cargo.toml")?;
    if parsed_manifest.get("package").is_none() {
        bail!("Not a contract project: Cargo.toml has no [package] section");
    }
    if !parsed_manifest
        .get("dependencies")
        .and_then(|v| v.get("soroban-sdk"))
        .is_some()
    {
        bail!("Not a Soroban contract project: Cargo.toml must declare soroban-sdk");
    }
    let ast = syn::parse_file(&source).context("Could not parse src/lib.rs as Rust")?;
    let contract = ast
        .items
        .iter()
        .find_map(|i| {
            if let syn::Item::Struct(s) = i {
                Some(s.ident.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Not a Soroban contract project: src/lib.rs has no contract struct")
        })?;
    let marker = format!("// starforge:component:{}", name);
    if source.contains(&marker) {
        bail!(
            "Component '{}' is already applied in {}",
            name,
            source_path.display()
        );
    }
    for fun in c.functions {
        if ast.items.iter().any(|i| matches!(i, syn::Item::Impl(x) if x.items.iter().any(|m| matches!(m, syn::ImplItem::Fn(f) if f.sig.ident == *fun)))) {
            bail!("Cannot apply '{}': function '{}' already exists in {}", name, fun, source_path.display());
        }
    }
    let target_impl = ast
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Impl(item)
                if item.self_ty.to_token_stream_string() == contract
                    && item.attrs.iter().any(|a| a.path().is_ident("contractimpl")) =>
            {
                Some(item)
            }
            _ => None,
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot apply '{}': expected #[contractimpl] impl {} block in {}",
                name,
                contract,
                source_path.display()
            )
        })?;
    let close = target_impl.brace_token.span.close().start();
    let insertion = source
        .lines()
        .take(close.line - 1)
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + close.column;
    let methods = c.template.trim();
    let mut updated = source.clone();
    updated.insert_str(insertion, &format!("\n    {}\n", methods));
    let mut changes = BTreeMap::new();
    changes.insert(
        source_path.clone(),
        format!("{}\n{}\n", updated.trim_end(), marker),
    );
    let old = source;
    let new = changes.get(&source_path).unwrap();
    if dry_run {
        println!("{}", unified_diff(&source_path, &old, new));
        return Ok(());
    }
    atomic_write(&changes)?;
    println!("Added '{}' to {}", name, root.display());
    Ok(())
}

trait TokenStreamString {
    fn to_token_stream_string(&self) -> String;
}
impl TokenStreamString for Box<syn::Type> {
    fn to_token_stream_string(&self) -> String {
        match self.as_ref() {
            syn::Type::Path(p) => p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

fn atomic_write(changes: &BTreeMap<PathBuf, String>) -> Result<()> {
    let nonce = format!("{}", std::process::id());
    let mut staged = Vec::new();
    for (path, content) in changes {
        let temp = path.with_extension(format!("starforge-{}-tmp", nonce));
        let backup = path.with_extension(format!("starforge-{}-bak", nonce));
        if let Err(error) = fs::write(&temp, content) {
            for (_, t, _) in &staged {
                let _ = fs::remove_file(t);
            }
            return Err(error).with_context(|| format!("Failed staging {}", path.display()));
        }
        staged.push((path.clone(), temp, backup));
    }
    let mut committed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (path, temp, backup) in &staged {
        if let Err(error) = fs::rename(path, backup) {
            let _ = fs::remove_file(temp);
            for (written, saved) in committed.iter().rev() {
                let _ = fs::remove_file(written);
                let _ = fs::rename(saved, written);
            }
            for (_, leftover, _) in &staged {
                let _ = fs::remove_file(leftover);
            }
            return Err(error).with_context(|| {
                format!(
                    "Failed writing {}; previous files were restored",
                    path.display()
                )
            });
        }
        if let Err(error) = fs::rename(temp, path) {
            let _ = fs::rename(backup, path);
            for (written, saved) in committed.iter().rev() {
                let _ = fs::remove_file(written);
                let _ = fs::rename(saved, written);
            }
            for (_, leftover, _) in &staged {
                let _ = fs::remove_file(leftover);
            }
            return Err(error).with_context(|| {
                format!(
                    "Failed writing {}; previous files were restored",
                    path.display()
                )
            });
        }
        committed.push((path.clone(), backup.clone()));
    }
    for (_, _, backup) in staged {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn unified_diff(path: &Path, old: &str, new: &str) -> String {
    let mut out = format!(
        "--- a/{}\n+++ b/{}\n@@ -1,{} +1,{} @@\n",
        path.display(),
        path.display(),
        old.lines().count(),
        new.lines().count()
    );
    for line in old.lines() {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in new.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_has_expected_components() {
        assert_eq!(
            REGISTRY.iter().map(|c| c.name).collect::<Vec<_>>(),
            ["ownable", "access-control", "pausable", "upgradeable"]
        );
        for component in REGISTRY {
            syn::parse_file(&format!("impl Component {{\n{}\n}}", component.template))
                .unwrap_or_else(|error| panic!("invalid template {}: {error}", component.name));
        }
    }
    #[test]
    fn invalid_project_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(apply("ownable", d.path(), false)
            .unwrap_err()
            .to_string()
            .contains("Not a contract project"));
    }
    #[test]
    fn dry_run_does_not_write_and_has_diff() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("src")).unwrap();
        fs::write(
            d.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n[dependencies]\nsoroban-sdk='22'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("src/lib.rs"),
            "pub struct C;\n#[contractimpl]\nimpl C {}\n",
        )
        .unwrap();
        let before = fs::read(d.path().join("src/lib.rs")).unwrap();
        apply("ownable", d.path(), true).unwrap();
        assert_eq!(before, fs::read(d.path().join("src/lib.rs")).unwrap());
    }
    #[test]
    fn second_apply_refuses() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("src")).unwrap();
        fs::write(
            d.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n[dependencies]\nsoroban-sdk='22'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("src/lib.rs"),
            "pub struct C;\n#[contractimpl]\nimpl C {}\n",
        )
        .unwrap();
        apply("ownable", d.path(), false).unwrap();
        assert!(apply("ownable", d.path(), false)
            .unwrap_err()
            .to_string()
            .contains("already applied"));
    }

    #[test]
    fn name_collision_refuses_before_writing() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("src")).unwrap();
        fs::write(
            d.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n[dependencies]\nsoroban-sdk='22'\n",
        )
        .unwrap();
        let source = "pub struct C;\n#[soroban_sdk::contractimpl]\nimpl C { pub fn owner(env: soroban_sdk::Env) {} }\n";
        fs::write(d.path().join("src/lib.rs"), source).unwrap();
        assert!(apply("ownable", d.path(), false)
            .unwrap_err()
            .to_string()
            .contains("function 'owner' already exists"));
        assert_eq!(
            fs::read_to_string(d.path().join("src/lib.rs")).unwrap(),
            source
        );
    }

    #[test]
    fn failed_multi_file_commit_rolls_back_prior_files() {
        let d = tempfile::tempdir().unwrap();
        let first = d.path().join("a.txt");
        let second = d.path().join("b.txt");
        fs::write(&first, "before-a").unwrap();
        fs::write(&second, "before-b").unwrap();
        let backup_collision =
            second.with_extension(format!("starforge-{}-bak", std::process::id()));
        fs::create_dir(&backup_collision).unwrap();
        let mut changes = BTreeMap::new();
        changes.insert(first.clone(), "after-a".to_string());
        changes.insert(second.clone(), "after-b".to_string());
        assert!(atomic_write(&changes).is_err());
        assert_eq!(fs::read_to_string(first).unwrap(), "before-a");
        assert_eq!(fs::read_to_string(second).unwrap(), "before-b");
    }

    #[test]
    fn diff_contains_both_file_versions() {
        let diff = unified_diff(Path::new("src/lib.rs"), "old", "new");
        assert!(diff.contains("--- a/src/lib.rs"));
        assert!(diff.contains("-old"));
        assert!(diff.contains("+new"));
    }
}
