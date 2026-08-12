use console::style;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};

use super::CliError;
use super::oneshot::validate_project_name;
use super::output;
use crate::registry::loader::Registry;
use crate::registry::types::{Network, Role, RoleAssignment, Selection};

/// Run the full interactive wizard, returning a validated Selection.
///
/// `allow_experimental` pre-acknowledges experimental tools (the CLI's
/// `--allow-experimental`): when `false`, choosing an experimental tool triggers
/// an explicit confirm before it's kept (the interactive counterpart of the
/// one-shot gate).
pub fn run_interactive(
    registry: &Registry,
    allow_experimental: bool,
) -> Result<Selection, CliError> {
    let theme = ColorfulTheme::default();

    // Step 1: Welcome
    output::print_welcome();

    // Step 2: Tool selection — prompt one tool per role (default "(skip)"). No
    // separate role-picking step: skipping a role is choosing "(skip)" for it.
    // Choosing the same fullstack-capable tool for both on-chain and off-chain
    // collapses into a single `protocol/` component automatically (planner).
    let mut assignments = select_tools(&theme, registry)?;
    if assignments.is_empty() {
        return Err(CliError::NoRolesSelected);
    }

    // Experimental gate: unless pre-acknowledged with --allow-experimental,
    // require an explicit confirm before keeping any experimental tool. Declining
    // drops those assignments rather than aborting the whole wizard.
    if !allow_experimental {
        assignments = confirm_experimental(&theme, assignments, registry)?;
        if assignments.is_empty() {
            return Err(CliError::NoRolesSelected);
        }
    }

    // Step 3: Options
    let project_name = prompt_project_name(&theme)?;
    let network = prompt_network(&theme)?;
    let nix = Confirm::with_theme(&theme)
        .with_prompt("Set up Nix for dependency management?")
        .default(false)
        .interact()?;
    let selection = Selection {
        project_name,
        assignments,
        network,
        nix,
    };

    // Step 4: Summary + confirmation
    output::print_summary(&selection, registry);

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Generate project?")
        .default(true)
        .interact()?;

    if !confirmed {
        return Err(CliError::Aborted);
    }

    Ok(selection)
}

/// Prompt a tool for every role in canonical order. Non-infra roles use a
/// single-choice `Select` whose default is a leading **"(skip)"** entry, so a
/// role is left empty just by pressing enter; infrastructure (multi-tool) uses a
/// `MultiSelect` where selecting nothing means skip. There is no separate
/// role-picking step, and no fullstack prompt — assigning the same
/// fullstack-capable tool to on-chain and off-chain collapses into `protocol/`
/// downstream (TECH_SPEC §3.4).
fn select_tools(
    theme: &ColorfulTheme,
    registry: &Registry,
) -> Result<Vec<RoleAssignment>, CliError> {
    let mut assignments = Vec::new();

    for &role in Role::ALL {
        let tools = registry.tools_for_role(role);
        if tools.is_empty() {
            continue;
        }

        if role == Role::Infrastructure {
            let items: Vec<String> = tools
                .iter()
                .map(|t| {
                    format!(
                        "{}{} — {}",
                        t.name,
                        output::experimental_tag(t),
                        output::first_sentence(&t.description)
                    )
                })
                .collect();
            let selections = MultiSelect::with_theme(theme)
                .with_prompt(format!(
                    "Choose tools for {} (space to select, enter to confirm — none to skip):",
                    role
                ))
                .items(&items)
                .interact()?;

            for &idx in &selections {
                assignments.push(RoleAssignment {
                    role,
                    tool_id: tools[idx].id.clone(),
                });
            }
        } else {
            // Leading "(skip)" is the default; real tools follow at index 1..
            let mut items = vec!["(skip)".to_string()];
            items.extend(tools.iter().map(|t| {
                format!(
                    "{}{} — {}",
                    t.name,
                    output::experimental_tag(t),
                    output::first_sentence(&t.description)
                )
            }));
            let idx = Select::with_theme(theme)
                .with_prompt(format!("Choose a tool for {}:", role))
                .items(&items)
                .default(0)
                .interact()?;

            if idx > 0 {
                assignments.push(RoleAssignment {
                    role,
                    tool_id: tools[idx - 1].id.clone(),
                });
            }
        }
    }

    Ok(assignments)
}

/// Gate experimental tools behind an explicit confirm (default No). Returns the
/// assignments to keep: if the user declines, the experimental ones are dropped
/// (with a note) and the rest proceed. Non-experimental selections pass through
/// untouched. This mirrors the one-shot `--allow-experimental` requirement.
fn confirm_experimental(
    theme: &ColorfulTheme,
    assignments: Vec<RoleAssignment>,
    registry: &Registry,
) -> Result<Vec<RoleAssignment>, CliError> {
    let experimental_names: Vec<&str> = assignments
        .iter()
        .filter_map(|a| registry.get(&a.tool_id))
        .filter(|t| t.experimental)
        .map(|t| t.name.as_str())
        .collect();

    if experimental_names.is_empty() {
        return Ok(assignments);
    }

    println!();
    println!(
        "  {} {} {}",
        style("⚠ Experimental:").yellow().bold(),
        style(experimental_names.join(", ")).yellow().bold(),
        style("— may be unstable or incomplete (rough edges, breaking changes).").yellow()
    );

    let include = Confirm::with_theme(theme)
        .with_prompt("Include this experimental tool anyway?")
        .default(false)
        .interact()?;

    if include {
        return Ok(assignments);
    }

    // Declined: drop every experimental assignment, keep the rest.
    println!(
        "  {}",
        style(format!(
            "Skipping experimental: {}",
            experimental_names.join(", ")
        ))
        .dim()
    );
    Ok(assignments
        .into_iter()
        .filter(|a| !registry.get(&a.tool_id).is_some_and(|t| t.experimental))
        .collect())
}

fn prompt_project_name(theme: &ColorfulTheme) -> Result<String, CliError> {
    let name: String = Input::with_theme(theme)
        .with_prompt("Project name")
        .default("my-protocol".to_string())
        .validate_with(|input: &String| -> Result<(), String> {
            validate_project_name(input).map_err(|e| e.to_string())
        })
        .interact_text()?;
    Ok(name)
}

fn prompt_network(theme: &ColorfulTheme) -> Result<Network, CliError> {
    let items = ["preview", "preprod", "mainnet"];
    let idx = Select::with_theme(theme)
        .with_prompt("Target network")
        .items(&items)
        .default(0)
        .interact()?;
    Ok(Network::from_str(items[idx]).expect("hardcoded network values are valid"))
}
