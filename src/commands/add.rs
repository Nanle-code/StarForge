use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use std::fs;
use std::path::PathBuf;

const METHODS_ANCHOR: &str = "// <starforge:add:methods>";
const PAUSE_ANCHOR: &str = "// <starforge:pause-check>";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum Component {
    AccessControl,
    Pausability,
    Upgradeability,
    Ownership,
}

impl Component {
    fn key(self) -> &'static str {
        match self {
            Self::AccessControl => "access-control",
            Self::Pausability => "pausability",
            Self::Upgradeability => "upgradeability",
            Self::Ownership => "ownership",
        }
    }

    fn methods(self) -> &'static str {
        match self {
            Self::AccessControl => {
                r#"    pub fn initialize_access_control(env: soroban_sdk::Env, admin: soroban_sdk::Address) {
        admin.require_auth();
        let admin_key = soroban_sdk::symbol_short!("AccessAdm");
        if env.storage().instance().has(&admin_key) {
            panic!("access control already initialized");
        }
        env.storage().instance().set(&admin_key, &admin);
    }

    pub fn set_authorized(env: soroban_sdk::Env, admin: soroban_sdk::Address, account: soroban_sdk::Address, authorized: bool) {
        let stored_admin: soroban_sdk::Address = env.storage().instance()
            .get(&soroban_sdk::symbol_short!("AccessAdm"))
            .unwrap_or_else(|| panic!("access control is not initialized"));
        if stored_admin != admin {
            panic!("not access control admin");
        }
        admin.require_auth();
        let key = soroban_sdk::symbol_short!("Allowed");
        let mut allowed: soroban_sdk::Map<soroban_sdk::Address, bool> = env.storage().persistent()
            .get(&key)
            .unwrap_or(soroban_sdk::Map::new(&env));
        allowed.set(account, authorized);
        env.storage().persistent().set(&key, &allowed);
    }

    pub fn is_authorized(env: soroban_sdk::Env, account: soroban_sdk::Address) -> bool {
        let allowed: soroban_sdk::Map<soroban_sdk::Address, bool> = env.storage().persistent()
            .get(&soroban_sdk::symbol_short!("Allowed"))
            .unwrap_or(soroban_sdk::Map::new(&env));
        allowed.get(account).unwrap_or(false)
    }

    pub fn require_authorized(env: soroban_sdk::Env, account: soroban_sdk::Address) {
        if !Self::is_authorized(env, account) {
            panic!("account is not authorized");
        }
    }"#
            }
            Self::Pausability => {
                r#"    pub fn initialize_pause_admin(env: soroban_sdk::Env, admin: soroban_sdk::Address) {
        admin.require_auth();
        let admin_key = soroban_sdk::symbol_short!("PauseAdm");
        if env.storage().instance().has(&admin_key) {
            panic!("pausability already initialized");
        }
        env.storage().instance().set(&admin_key, &admin);
    }

    pub fn pause(env: soroban_sdk::Env, admin: soroban_sdk::Address) {
        let stored_admin: soroban_sdk::Address = env.storage().instance()
            .get(&soroban_sdk::symbol_short!("PauseAdm"))
            .unwrap_or_else(|| panic!("pausability is not initialized"));
        if stored_admin != admin {
            panic!("not pause admin");
        }
        admin.require_auth();
        env.storage().instance().set(&soroban_sdk::symbol_short!("Paused"), &true);
    }

    pub fn unpause(env: soroban_sdk::Env, admin: soroban_sdk::Address) {
        let stored_admin: soroban_sdk::Address = env.storage().instance()
            .get(&soroban_sdk::symbol_short!("PauseAdm"))
            .unwrap_or_else(|| panic!("pausability is not initialized"));
        if stored_admin != admin {
            panic!("not pause admin");
        }
        admin.require_auth();
        env.storage().instance().set(&soroban_sdk::symbol_short!("Paused"), &false);
    }

    pub fn is_paused(env: soroban_sdk::Env) -> bool {
        env.storage().instance().get(&soroban_sdk::symbol_short!("Paused")).unwrap_or(false)
    }

    pub fn assert_not_paused(env: soroban_sdk::Env) {
        if Self::is_paused(env) {
            panic!("contract is paused");
        }
    }"#
            }
            Self::Upgradeability => {
                r#"    pub fn initialize_upgrade_admin(env: soroban_sdk::Env, admin: soroban_sdk::Address) {
        admin.require_auth();
        let admin_key = soroban_sdk::symbol_short!("UpgAdmin");
        if env.storage().instance().has(&admin_key) {
            panic!("upgradeability already initialized");
        }
        env.storage().instance().set(&admin_key, &admin);
    }

    pub fn upgrade(env: soroban_sdk::Env, admin: soroban_sdk::Address, wasm_hash: soroban_sdk::BytesN<32>) {
        let stored_admin: soroban_sdk::Address = env.storage().instance()
            .get(&soroban_sdk::symbol_short!("UpgAdmin"))
            .unwrap_or_else(|| panic!("upgradeability is not initialized"));
        if stored_admin != admin {
            panic!("not upgrade admin");
        }
        admin.require_auth();
        env.deployer().update_current_contract_wasm(wasm_hash);
    }"#
            }
            Self::Ownership => {
                r#"    pub fn initialize_owner(env: soroban_sdk::Env, owner: soroban_sdk::Address) {
        owner.require_auth();
        let owner_key = soroban_sdk::symbol_short!("Owner");
        if env.storage().instance().has(&owner_key) {
            panic!("ownership already initialized");
        }
        env.storage().instance().set(&owner_key, &owner);
    }

    pub fn owner(env: soroban_sdk::Env) -> soroban_sdk::Address {
        env.storage().instance()
            .get(&soroban_sdk::symbol_short!("Owner"))
            .unwrap_or_else(|| panic!("ownership is not initialized"))
    }

    pub fn transfer_ownership(env: soroban_sdk::Env, current_owner: soroban_sdk::Address, new_owner: soroban_sdk::Address) {
        let stored_owner: soroban_sdk::Address = Self::owner(env.clone());
        if stored_owner != current_owner {
            panic!("not contract owner");
        }
        current_owner.require_auth();
        env.storage().instance().set(&soroban_sdk::symbol_short!("Owner"), &new_owner);
    }"#
            }
        }
    }

    fn method_names(self) -> &'static [&'static str] {
        match self {
            Self::AccessControl => &[
                "initialize_access_control",
                "set_authorized",
                "is_authorized",
                "require_authorized",
            ],
            Self::Pausability => &[
                "initialize_pause_admin",
                "pause",
                "unpause",
                "is_paused",
                "assert_not_paused",
            ],
            Self::Upgradeability => &["initialize_upgrade_admin", "upgrade"],
            Self::Ownership => &["initialize_owner", "owner", "transfer_ownership"],
        }
    }
}

#[derive(Args)]
pub struct AddArgs {
    /// Component to add: access-control, pausability, upgradeability, ownership
    component: Component,
    /// Project directory (defaults to the current directory)
    #[arg(long, default_value = ".")]
    path: PathBuf,
    /// Show the patch without writing changes
    #[arg(long)]
    dry_run: bool,
}

pub fn handle(args: AddArgs) -> Result<()> {
    add_to_path(args.component, &args.path, args.dry_run)
}

/// Apply `component` to `<project_dir>/src/lib.rs`.
///
/// On a dry run the unified diff is printed and nothing is written.
pub fn add_to_path(component: Component, project_dir: &std::path::Path, dry_run: bool) -> Result<()> {
    let source_path = project_dir.join("src").join("lib.rs");
    let source = fs::read_to_string(&source_path)
        .with_context(|| format!("Could not read {}", source_path.display()))?;
    let updated = apply_component(&source, component)?;

    if dry_run {
        print_diff(&source_path, &source, &updated);
    } else {
        fs::write(&source_path, updated)
            .with_context(|| format!("Could not write {}", source_path.display()))?;
        println!(
            "Added {} to {}",
            component.key(),
            source_path.display()
        );
    }
    Ok(())
}

fn apply_component(source: &str, component: Component) -> Result<String> {
    let syntax =
        syn::parse_file(source).context("src/lib.rs is not valid Rust; refusing to patch it")?;

    let component_marker = format!("// <starforge:component:{}>", component.key());
    if source.contains(&component_marker) {
        bail!("Component '{}' is already present", component.key());
    }

    let anchor_count = source.matches(METHODS_ANCHOR).count();
    if anchor_count != 1 {
        bail!("Expected exactly one StarForge add anchor in src/lib.rs; found {anchor_count}");
    }

    for item in syntax.items {
        if let syn::Item::Impl(item_impl) = item {
            for item in item_impl.items {
                if let syn::ImplItem::Fn(method) = item {
                    let method_name = method.sig.ident.to_string();
                    if component.method_names().contains(&method_name.as_str()) {
                        bail!(
                            "Cannot add '{}': method '{}' already exists",
                            component.key(),
                            method_name
                        );
                    }
                }
            }
        }
    }

    let marker_and_methods = format!(
        "{METHODS_ANCHOR}\n    {component_marker}\n{}",
        component.methods()
    );
    let mut updated = source.replacen(METHODS_ANCHOR, &marker_and_methods, 1);

    if component == Component::Pausability {
        // Only the anchor token is replaced, so the line keeps whatever
        // indentation it was written with and the statement lands in-column.
        updated = updated.replace(
            PAUSE_ANCHOR,
            "Self::assert_not_paused(env.clone());",
        );
    }

    syn::parse_file(&updated)
        .context("Generated component patch is not valid Rust; refusing to write")?;
    Ok(updated)
}

fn print_diff(path: &std::path::Path, old: &str, new: &str) {
    let old_lines: Vec<_> = old.lines().collect();
    let new_lines: Vec<_> = new.lines().collect();
    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old_lines.len().saturating_sub(prefix)
        && suffix < new_lines.len().saturating_sub(prefix)
        && old_lines[old_lines.len() - suffix - 1] == new_lines[new_lines.len() - suffix - 1]
    {
        suffix += 1;
    }

    let start = prefix.saturating_sub(3);
    let old_change_end = old_lines.len().saturating_sub(suffix);
    let new_change_end = new_lines.len().saturating_sub(suffix);
    let old_context_end = old_lines.len().min(old_change_end + 3);
    let new_context_end = new_lines.len().min(new_change_end + 3);
    println!("--- a/{}", path.display());
    println!("+++ b/{}", path.display());
    println!(
        "@@ -{},{} +{},{} @@",
        start + 1,
        old_context_end - start,
        start + 1,
        new_context_end - start
    );
    for line in &old_lines[start..prefix] {
        println!(" {line}");
    }
    for line in &old_lines[prefix..old_change_end] {
        println!("-{line}");
    }
    for line in &new_lines[prefix..new_change_end] {
        println!("+{line}");
    }
    for line in &old_lines[old_change_end..old_context_end] {
        println!(" {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::{add_to_path, apply_component, Component, METHODS_ANCHOR, PAUSE_ANCHOR};
    use crate::commands::new;
    use std::path::{Path, PathBuf};

    const COMPONENTS: [Component; 4] = [
        Component::AccessControl,
        Component::Pausability,
        Component::Upgradeability,
        Component::Ownership,
    ];

    /// Templates rendered inline by `starforge new`.
    fn inline_templates() -> Vec<(String, String)> {
        vec![
            (
                "hello-world".into(),
                new::hello_world_template("example", "persistent", true),
            ),
            ("hello-world-temporary".into(), new::hello_world_template("example", "temporary", true)),
            ("token".into(), new::token_template("example")),
            ("voting".into(), new::voting_template("example")),
            ("nft".into(), new::nft_template("example")),
        ]
    }

    /// Templates shipped under `templates/examples`, read from disk.
    fn example_templates() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/examples");
        let mut found = Vec::new();
        let entries = std::fs::read_dir(&root)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let lib = path.join("src").join("lib.rs");
            if !lib.exists() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let raw = std::fs::read_to_string(&lib)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", lib.display()));
            found.push((name.clone(), render_placeholders(&raw, &name)));
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        assert!(
            found.len() >= 10,
            "expected the built-in example templates to be present, found {}",
            found.len()
        );
        found
    }

    fn render_placeholders(source: &str, name: &str) -> String {
        let pascal: String = name
            .split(['-', '_'])
            .filter(|p| !p.is_empty())
            .map(|p| {
                let mut chars = p.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect();
        source
            .replace("{{PROJECT_NAME_PASCAL}}", &pascal)
            .replace("{{PROJECT_NAME_SNAKE}}", &name.replace('-', "_"))
            .replace("{{PROJECT_NAME}}", name)
    }

    /// Every component must apply cleanly, and the result must stay valid Rust,
    /// for every built-in template -- both inline and packaged.
    #[test]
    fn every_component_patches_every_builtin_template_as_valid_rust() {
        let mut templates = inline_templates();
        templates.extend(example_templates());

        for (template_name, source) in &templates {
            syn::parse_file(source)
                .unwrap_or_else(|e| panic!("{template_name} is not valid Rust: {e}"));

            for component in COMPONENTS {
                let patched = apply_component(source, component)
                    .unwrap_or_else(|e| panic!("{template_name} + {component:?}: {e}"));
                syn::parse_file(&patched)
                    .unwrap_or_else(|e| panic!("{template_name} + {component:?}: {e}"));
            }
        }
    }

    /// All four components must compose onto the same contract.
    #[test]
    fn all_components_compose_onto_one_contract() {
        for (name, source) in inline_templates().into_iter().chain(example_templates()) {
            let mut patched = source;
            for component in COMPONENTS {
                patched = apply_component(&patched, component)
                    .unwrap_or_else(|e| panic!("{name} + {component:?}: {e}"));
            }
            syn::parse_file(&patched).unwrap_or_else(|e| panic!("{name} all four: {e}"));
        }
    }

    /// `starforge add pausability` must consume every pause anchor and leave the
    /// source balanced. Regression test: a stray brace in the component body
    /// previously produced unparseable output on every template.
    #[test]
    fn pausability_replaces_every_pause_anchor_in_column() {
        for (name, source) in inline_templates().into_iter().chain(example_templates()) {
            let anchors = source.matches(PAUSE_ANCHOR).count();
            assert!(anchors > 0, "{name} has no pause anchors to rewrite");

            let patched = apply_component(&source, Component::Pausability)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                !patched.contains(PAUSE_ANCHOR),
                "{name} still has an un-rewritten pause anchor"
            );
            assert_eq!(
                patched.matches("Self::assert_not_paused(env.clone());").count(),
                anchors,
                "{name}: expected {anchors} pause checks, found {}",
                patched.matches("Self::assert_not_paused(env.clone());").count()
            );
            // The injected statement must sit at the method-body indent, not be
            // double-indented by a replacement that carries its own padding.
            // Matched per line rather than on a literal "\n        ..." so the
            // check holds for CRLF checkouts on Windows.
            assert!(
                patched.lines().any(|line| {
                    line.trim() == "Self::assert_not_paused(env.clone());"
                        && line.len() - line.trim_start().len() == 8
                }),
                "{name}: pause check is not indented to the method body"
            );
        }
    }

    /// A non-pausability component must leave the pause anchors untouched.
    #[test]
    fn other_components_leave_pause_anchors_alone() {
        for (name, source) in inline_templates().into_iter().chain(example_templates()) {
            for component in [
                Component::AccessControl,
                Component::Upgradeability,
                Component::Ownership,
            ] {
                let patched = apply_component(&source, component)
                    .unwrap_or_else(|e| panic!("{name} + {component:?}: {e}"));
                assert_eq!(
                    patched.matches(PAUSE_ANCHOR).count(),
                    source.matches(PAUSE_ANCHOR).count(),
                    "{name} + {component:?} should not touch pause anchors"
                );
            }
        }
    }

    #[test]
    fn refuses_missing_anchors_and_duplicate_components() {
        assert!(apply_component("#![no_std]\n", Component::Ownership).is_err());

        for (name, source) in inline_templates().into_iter().chain(example_templates()) {
            let patched = apply_component(&source, Component::Ownership)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let err = apply_component(&patched, Component::Ownership)
                .err()
                .unwrap_or_else(|| panic!("{name}: adding ownership twice should be refused"));
            assert!(
                err.to_string().contains("already present"),
                "{name}: unexpected error: {err}"
            );
        }
    }

    #[test]
    fn refuses_method_name_conflicts() {
        let source = new::hello_world_template("example", "none", true).replace(
            METHODS_ANCHOR,
            "    // <starforge:add:methods>\n    pub fn owner(env: Env) -> Symbol { symbol_short!(\"Own\") }",
        );
        let err = apply_component(&source, Component::Ownership).expect_err("expected conflict");
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[test]
    fn refuses_source_that_is_not_valid_rust() {
        let err = apply_component("fn broken( {", Component::Ownership).expect_err("expected parse failure");
        assert!(
            err.to_string().contains("not valid Rust"),
            "got: {err}"
        );
    }

    #[test]
    fn refuses_duplicate_and_missing_anchor_counts() {
        let doubled = format!("{METHODS_ANCHOR}\n{METHODS_ANCHOR}");
        assert!(apply_component(&doubled, Component::Ownership).is_err());
    }

    /// `add_to_path` must write exactly one component marker and leave the file
    /// parseable, and a dry run must not touch the file at all.
    #[test]
    fn add_to_path_writes_once_and_dry_run_leaves_file_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path();
        std::fs::create_dir_all(project.join("src")).unwrap();
        let lib = project.join("src").join("lib.rs");
        let original = new::token_template("example");
        std::fs::write(&lib, &original).unwrap();

        let before = std::fs::read_to_string(&lib).unwrap();
        add_to_path(Component::Ownership, project, true).expect("dry run");
        assert_eq!(
            std::fs::read_to_string(&lib).unwrap(),
            before,
            "dry run must not modify the file"
        );

        add_to_path(Component::Ownership, project, false).expect("apply");
        let after = std::fs::read_to_string(&lib).unwrap();
        assert!(after.contains("// <starforge:component:ownership>"));
        assert_ne!(after, before);
        syn::parse_file(&after).expect("patched file must be valid Rust");

        // Re-applying the same component to the already-patched project is refused.
        assert!(add_to_path(Component::Ownership, project, false).is_err());
    }

    #[test]
    fn add_to_path_reports_a_missing_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = add_to_path(Component::Ownership, &dir.path().join("nope"), false)
            .expect_err("expected a missing project error");
        assert!(err.to_string().contains("Could not read"), "got: {err}");
    }

    /// Guards the packaged templates: every one must carry exactly one methods
    /// anchor, or `starforge add` cannot be used on it.
    #[test]
    fn every_packaged_template_has_exactly_one_methods_anchor() {
        for (name, source) in example_templates() {
            assert_eq!(
                source.matches(METHODS_ANCHOR).count(),
                1,
                "templates/examples/{name} must contain exactly one `{METHODS_ANCHOR}`"
            );
        }
    }

    #[test]
    fn example_templates_resolve_from_the_manifest_dir() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/examples");
        assert!(root.is_dir(), "{} should exist", root.display());
    }
}
