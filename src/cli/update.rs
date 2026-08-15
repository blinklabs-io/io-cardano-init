//! The `add` / `remove` / `edit` commands: edit an existing project's role/tool
//! set in place. Impure edge — reconstructs the current selection from the tree
//! (via `doctor::probe`), mutates it, then diffs and writes with `scaffold`.
//!
//! Reuses the init gates verbatim (compat + experimental) so an update is held
//! to exactly the same rules as a fresh scaffold.

use std::path::{Path, PathBuf};

use super::oneshot::{validate_fullstack_tool, validate_tool_for_role};
use super::{AddArgs, CliError, Format, RemoveArgs, UpdateFlags, output, theme};
use crate::doctor::probe;
use crate::registry::loader::Registry;
use crate::registry::types::{Role, RoleAssignment, Selection};
use crate::scaffold::update::{self, Mutation, SlotOp};

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

/// `cardano-init add …` — assign/replace tools in the current project.
pub fn run_add(args: AddArgs, registry: &Registry, format: Format) -> Result<(), CliError> {
    // `--fullstack X` is sugar for on-chain + off-chain X; not combinable.
    if args.fullstack.is_some() && (args.on_chain.is_some() || args.off_chain.is_some()) {
        return Err(CliError::FullstackConflict);
    }

    let mut mutations = Vec::new();
    if let Some(id) = &args.fullstack {
        validate_fullstack_tool(id, registry)?;
        mutations.push(add(Role::OnChain, id));
        mutations.push(add(Role::OffChain, id));
    }
    if let Some(id) = &args.on_chain {
        validate_tool_for_role(id, Role::OnChain, registry)?;
        mutations.push(add(Role::OnChain, id));
    }
    if let Some(id) = &args.off_chain {
        validate_tool_for_role(id, Role::OffChain, registry)?;
        mutations.push(add(Role::OffChain, id));
    }
    for id in &args.infra {
        validate_tool_for_role(id, Role::Infrastructure, registry)?;
        mutations.push(add(Role::Infrastructure, id));
    }
    if let Some(id) = &args.devnet {
        validate_tool_for_role(id, Role::Devnet, registry)?;
        mutations.push(add(Role::Devnet, id));
    }
    if let Some(id) = &args.formal_methods {
        validate_tool_for_role(id, Role::FormalMethods, registry)?;
        mutations.push(add(Role::FormalMethods, id));
    }

    let cwd = current_dir();
    let old = detect_selection(&cwd, registry, format)?;
    let new = update::apply_all(&old, &mutations);
    finish_update(&cwd, old, new, &args.flags, registry, format)
}

/// `cardano-init remove …` — drop roles / infra providers from the project.
pub fn run_remove(args: RemoveArgs, registry: &Registry, format: Format) -> Result<(), CliError> {
    let mut mutations = Vec::new();
    if args.on_chain {
        mutations.push(Mutation::Remove(Role::OnChain));
    }
    if args.off_chain {
        mutations.push(Mutation::Remove(Role::OffChain));
    }
    if args.devnet {
        mutations.push(Mutation::Remove(Role::Devnet));
    }
    if args.formal_methods {
        mutations.push(Mutation::Remove(Role::FormalMethods));
    }
    for id in &args.infra {
        mutations.push(Mutation::RemoveInfra(id.clone()));
    }

    let cwd = current_dir();
    let old = detect_selection(&cwd, registry, format)?;
    let new = update::apply_all(&old, &mutations);
    finish_update(&cwd, old, new, &args.flags, registry, format)
}

// ---------------------------------------------------------------------------
// Shared flow
// ---------------------------------------------------------------------------

fn add(role: Role, tool_id: &str) -> Mutation {
    Mutation::Add(RoleAssignment {
        role,
        tool_id: tool_id.to_string(),
    })
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Reconstruct the current selection, refusing to guess when the tree has
/// unrecognized components (fatal in non-interactive/JSON; a confirm otherwise).
fn detect_selection(
    cwd: &Path,
    registry: &Registry,
    format: Format,
) -> Result<Selection, CliError> {
    let recon = probe::reconstruct(cwd, registry);

    let mut problems: Vec<String> = recon.unrecognized.iter().map(|u| u.dir.clone()).collect();
    problems.extend(recon.unknown_infra.iter().cloned());

    if !problems.is_empty() {
        // Automation must not act on a guess.
        if format == Format::Json {
            return Err(CliError::ProjectUnrecognized { dirs: problems });
        }
        println!(
            "  {}  these look off and will be ignored: {}",
            theme::badge_warn("HEADS UP"),
            problems.join(", ")
        );
        if !confirm("Continue with the detected stack anyway?", false)? {
            return Err(CliError::Aborted);
        }
    }

    if format == Format::Human && !recon.low_confidence.is_empty() {
        println!(
            "  {}  guessed (couldn't read from the project): {}",
            theme::dim("note:"),
            recon.low_confidence.join(", ")
        );
    }

    Ok(recon.selection)
}

/// Validate the mutated selection, then plan → (dry-run | git-gate + confirm +
/// apply) → report. Shared by add/remove/edit.
fn finish_update(
    cwd: &Path,
    old: Selection,
    new: Selection,
    flags: &UpdateFlags,
    registry: &Registry,
    format: Format,
) -> Result<(), CliError> {
    if new == old {
        return Err(CliError::NothingToChange);
    }
    if new.assignments.is_empty() {
        return Err(CliError::NoRolesSelected);
    }

    // Same gates a fresh scaffold runs, on the resulting selection.
    if !flags.allow_experimental {
        super::experimental_gate(&new, registry)?;
    }
    if let Some(inc) = crate::registry::compat::check(&new.assignments, registry) {
        if flags.ignore_warning {
            if format == Format::Human {
                output::print_incompatibility_warning(&inc);
            }
        } else {
            return Err(super::incompatible_tools_error(inc));
        }
    }

    let plan = update::plan_update(&old, &new, registry)?;

    // A create target must not already exist (a foreign/renamed dir would have
    // been flagged unrecognized; this is the last-line guard).
    for op in &plan.slot_ops {
        if let SlotOp::Create(dir) = op
            && cwd.join(dir).exists()
        {
            let name = dir.to_string_lossy().into_owned();
            return Err(CliError::SlotOccupied {
                role: name.clone(),
                dir: name,
            });
        }
    }

    if flags.dry_run {
        output::print_update_plan(&new.project_name, &plan, format);
        return Ok(());
    }

    // Git safety net: the change must be reviewable/revertible.
    if !flags.force && !super::git::is_clean(cwd) {
        return Err(CliError::WorktreeDirty {
            path: cwd.display().to_string(),
        });
    }

    // Human confirm: show the change set, then ask.
    if format == Format::Human {
        output::print_update_plan(&new.project_name, &plan, format);
        if !confirm("Apply this update?", true)? {
            return Err(CliError::Aborted);
        }
    }

    crate::scaffold::writer::apply_update(&plan, cwd)?;

    let report = super::resolve_selection_deps(&new, registry)?;
    output::print_update_success(&new, registry, &plan, &report, format);
    Ok(())
}

fn confirm(prompt: &str, default: bool) -> Result<bool, CliError> {
    dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt(prompt)
        .default(default)
        .interact()
        .map_err(CliError::from)
}
