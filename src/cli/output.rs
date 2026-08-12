use console::style;
use serde_json::json;

use super::{CliError, Format};
use crate::doctor::Report;
use crate::doctor::installers::Installer;
use crate::doctor::probe::{Environment, ScanResult};
use crate::registry::loader::Registry;
use crate::registry::types::{Role, Selection, ToolDef};
use crate::scaffold::planner::FilePlan;

// ---------------------------------------------------------------------------
// JSON envelope (TECH_SPEC §2.4)
// ---------------------------------------------------------------------------

const SCHEMA_VERSION: u32 = 1;

/// Print a success envelope to stdout: `{ schema_version, ok: true, data }`.
fn emit_json_ok(data: serde_json::Value) {
    let envelope = json!({ "schema_version": SCHEMA_VERSION, "ok": true, "data": data });
    println!(
        "{}",
        serde_json::to_string(&envelope).expect("envelope serializes")
    );
}

/// Render any error in the requested format.
///
/// In `json`, an error envelope `{ ok: false, error: { code, message, context } }`
/// is written to stderr. In `human`, the styled `error: …` line is written to
/// stderr — except for an interactive abort (exit code 0), which is silent.
pub fn print_error(err: &CliError, format: Format) {
    match format {
        Format::Json => {
            let envelope = json!({
                "schema_version": SCHEMA_VERSION,
                "ok": false,
                "error": {
                    "code": err.code(),
                    "message": err.to_string(),
                    "context": err.context(),
                },
            });
            eprintln!(
                "{}",
                serde_json::to_string(&envelope).expect("envelope serializes")
            );
        }
        Format::Human => {
            if err.exit_code() != 0 {
                eprintln!("{}: {}", style("error").red().bold(), err);
            }
        }
    }
}

/// Print the welcome banner for interactive mode.
pub fn print_welcome() {
    println!();
    println!(
        "  {} Let's set up your Cardano protocol project.",
        style("Welcome to cardano-init!").bold()
    );
    println!();
    println!("  A Cardano protocol typically has up to five components:");
    println!(
        "  {} Smart contract logic (validators) that runs on the ledger",
        style("On-chain:").cyan().bold()
    );
    println!(
        "  {} Code that builds and submits transactions",
        style("Off-chain:").cyan().bold()
    );
    println!(
        "  {} Indexers and services that read chain data",
        style("Infrastructure:").cyan().bold()
    );
    println!(
        "  {} Local throwaway chain to develop and test against",
        style("Devnet:").cyan().bold()
    );
    println!(
        "  {} Specification and automated verification tools",
        style("Formal methods:").cyan().bold()
    );
    println!();
}

/// The inline tag appended after an experimental tool's name in list-style
/// output (`""` for released tools). Kept here so `list`, `--help`, and the
/// interactive prompts render the marker identically.
pub fn experimental_tag(tool: &ToolDef) -> &'static str {
    if tool.experimental {
        " [experimental]"
    } else {
        ""
    }
}

/// The distinct experimental tools in a selection, in role order and
/// de-duplicated (a fullstack tool assigned to both roles is reported once).
/// Shared by the generation-time warning and the `--allow-experimental` gate.
pub fn selected_experimental_tools<'a>(
    selection: &Selection,
    registry: &'a Registry,
) -> Vec<&'a ToolDef> {
    let mut out: Vec<&ToolDef> = Vec::new();
    for a in &selection.assignments {
        if let Some(tool) = registry.get(&a.tool_id)
            && tool.experimental
            && !out.iter().any(|t| t.id == tool.id)
        {
            out.push(tool);
        }
    }
    out
}

/// Warn — loudly — when the selection includes any experimental tool, so a user
/// never generates an unstable/incomplete tool without knowing it. Printed inside
/// the pre-generation summary and again on success. (The hard gate is
/// `--allow-experimental` / the interactive confirm; this is the reminder that
/// fires once the tool is allowed through.) Silent when nothing experimental is
/// selected.
fn print_experimental_warning(selection: &Selection, registry: &Registry) {
    let experimental_tools = selected_experimental_tools(selection, registry);
    if experimental_tools.is_empty() {
        return;
    }

    let names = experimental_tools
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    println!();
    println!(
        "  {} {}",
        style("⚠ Experimental:").yellow().bold(),
        style(names).yellow().bold()
    );
    println!(
        "    {}",
        style("These tools may be unstable or incomplete (the tool itself and/or its").yellow()
    );
    println!(
        "    {}",
        style("integration). Expect rough edges and breaking changes.").yellow()
    );
}

/// Print a summary of the selection before generation.
pub fn print_summary(selection: &Selection, registry: &Registry) {
    println!();
    println!("  {}", style("Summary").bold().underlined());
    println!();
    println!("  Project:  {}", style(&selection.project_name).cyan());

    for component in components(selection, registry) {
        let role_label = component_label(&component.kebab);

        let (tool_info, experimental) = if let Some(tool) = registry.get(&component.tool_id) {
            // Infra providers carry no user-facing language, so omit the
            // parenthetical rather than printing an empty "(?)".
            let info = match tool.languages.first() {
                Some(lang) => format!("{} ({})", tool.name, lang),
                None => tool.name.clone(),
            };
            (info, tool.experimental)
        } else {
            (component.tool_id.clone(), false)
        };

        let styled = if experimental {
            style(format!("{tool_info}  [experimental]")).yellow()
        } else {
            style(tool_info).cyan()
        };
        println!("  {:<12}{}", format!("{}:", role_label), styled);
    }

    println!("  Network:  {}", style(&selection.network).cyan());

    if selection.nix {
        println!("  Nix:      {}", style("yes").green());
    }

    print_experimental_warning(selection, registry);
    println!();
}

/// A generated component as reported to the user: its role (kebab) and tool id.
struct Component {
    /// Role kebab, or `"protocol"` for a collapsed fullstack component.
    kebab: String,
    tool_id: String,
}

/// The components actually scaffolded, with the fullstack on-chain+off-chain pair
/// folded into a single `protocol` component (matching the generated tree). This
/// is what the summary and JSON report, so they reflect reality rather than the
/// two raw assignments.
fn components(selection: &Selection, registry: &Registry) -> Vec<Component> {
    let fullstack_id = crate::scaffold::planner::fullstack_tool_id(selection, registry);
    let mut out = Vec::new();
    for a in &selection.assignments {
        let is_fullstack_member = fullstack_id.as_deref() == Some(a.tool_id.as_str())
            && matches!(a.role, Role::OnChain | Role::OffChain);
        if is_fullstack_member {
            // Emit one `protocol` entry (on the on-chain half); skip the off-chain.
            if a.role == Role::OffChain {
                continue;
            }
            out.push(Component {
                kebab: crate::contract::DIR_PROTOCOL.to_string(),
                tool_id: a.tool_id.clone(),
            });
            continue;
        }
        out.push(Component {
            kebab: a.role.as_kebab().to_string(),
            tool_id: a.tool_id.clone(),
        });
    }
    out
}

/// The human, title-cased label for a component's role kebab (used in summaries).
fn component_label(kebab: &str) -> &'static str {
    match kebab {
        "on-chain" => "On-chain",
        "off-chain" => "Off-chain",
        "protocol" => "Protocol",
        "infrastructure" => "Infra",
        "devnet" => "Devnet",
        "formal-methods" => "Formal methods",
        _ => "Component",
    }
}

/// Build the `[{ role, tool }]` component list for a selection (collapse-aware).
fn components_json(selection: &Selection, registry: &Registry) -> serde_json::Value {
    let items: Vec<serde_json::Value> = components(selection, registry)
        .iter()
        .map(|c| {
            // `experimental` is additive: it signals to agents that a generated
            // component is an experimental tool.
            let experimental = registry.get(&c.tool_id).is_some_and(|t| t.experimental);
            json!({ "role": c.kebab, "tool": c.tool_id, "experimental": experimental })
        })
        .collect();
    serde_json::Value::Array(items)
}

/// Print the dry-run output: summary + nested file tree (human), or the planned
/// file list (json).
pub fn print_dry_run(selection: &Selection, registry: &Registry, plan: &FilePlan, format: Format) {
    if format == Format::Json {
        let files: Vec<&str> = plan
            .entries
            .iter()
            .map(|e| e.dest.to_str().expect("paths are UTF-8"))
            .collect();
        emit_json_ok(json!({
            "project": selection.project_name,
            "network": selection.network.to_string(),
            "nix": selection.nix,
            "dry_run": true,
            "generated": false,
            "components": components_json(selection, registry),
            "files": files,
        }));
        return;
    }

    print_summary(selection, registry);

    println!("  {}", style(format!("{}/", selection.project_name)).bold());

    let paths: Vec<Vec<&str>> = plan
        .entries
        .iter()
        .map(|e| {
            e.dest
                .to_str()
                .expect("paths are UTF-8")
                .split('/')
                .collect()
        })
        .collect();

    print_tree(&paths, 0, 0, &mut String::new());

    println!();
    println!(
        "  {} files would be generated.",
        style(plan.entries.len()).bold()
    );
    println!();
}

/// Recursively print a directory tree from a sorted list of split paths.
///
/// `paths` contains only entries whose prefix (components 0..depth) matches
/// the current branch. `depth` is the current tree level. `indent` is the
/// prefix string built from the ancestors' box-drawing connectors.
fn print_tree(paths: &[Vec<&str>], depth: usize, _start: usize, indent: &mut String) {
    // Group entries by their component at `depth`.
    // Preserve insertion order so the tree follows the plan order.
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if depth >= path.len() {
            continue;
        }
        let key = path[depth];
        if let Some(group) = groups.iter_mut().find(|(k, _)| *k == key) {
            group.1.push(i);
        } else {
            groups.push((key, vec![i]));
        }
    }

    let total = groups.len();
    for (gi, (name, indices)) in groups.iter().enumerate() {
        let is_last = gi == total - 1;
        let connector = if is_last { "└── " } else { "├── " };

        // Check if this is a directory (has children deeper than depth+1)
        let is_dir = indices.iter().any(|&i| paths[i].len() > depth + 1);

        if is_dir {
            println!(
                "  {}{}{}",
                indent,
                style(connector).dim(),
                style(format!("{name}/")).dim()
            );
        } else {
            println!("  {}{}{}", indent, style(connector).dim(), name);
        }

        // Recurse into children that have more components
        let children: Vec<Vec<&str>> = indices
            .iter()
            .filter(|&&i| paths[i].len() > depth + 1)
            .map(|&i| paths[i].clone())
            .collect();

        if !children.is_empty() {
            let extension = if is_last { "    " } else { "│   " };
            let prev_len = indent.len();
            indent.push_str(extension);
            print_tree(&children, depth + 1, 0, indent);
            indent.truncate(prev_len);
        }
    }
}

/// Print success after scaffolding, including the dependency check-and-advise
/// (TECH_SPEC §9). In `json`, emits one envelope carrying the selection plus the
/// dependency report.
pub fn print_success(selection: &Selection, registry: &Registry, report: &Report, format: Format) {
    if format == Format::Json {
        emit_json_ok(json!({
            "project": selection.project_name,
            "network": selection.network.to_string(),
            "nix": selection.nix,
            "generated": true,
            "components": components_json(selection, registry),
            "dependencies": report,
        }));
        return;
    }

    println!();
    println!(
        "  {} Created {}",
        style("✔").green().bold(),
        style(&selection.project_name).cyan().bold()
    );

    for component in components(selection, registry) {
        let experimental = registry
            .get(&component.tool_id)
            .is_some_and(|t| t.experimental);
        let tag = if experimental {
            style("  [experimental]").yellow().bold().to_string()
        } else {
            String::new()
        };
        println!(
            "  {} Scaffolded {} ({}){}",
            style("✔").green().bold(),
            component.kebab,
            component.tool_id,
            tag
        );
    }

    // Reiterate the experimental caveat after generation (a one-shot user may
    // not have seen the pre-generation summary warning scroll past).
    print_experimental_warning(selection, registry);

    // Check-and-advise: surface any missing required deps before "Next steps".
    print_dep_advice(report);

    println!();
    println!("  {}", style("Next steps:").bold());
    println!("    cd {}", selection.project_name);
    if !report.all_required_present {
        println!("    # install the missing dependencies listed above, then:");
    }
    println!("    just build");
    println!();
}

/// Print the dependency advice block (missing required deps + install plans).
/// Silent when everything required is already present.
fn print_dep_advice(report: &Report) {
    if report.all_required_present {
        println!();
        println!(
            "  {} All required dependencies are installed.",
            style("✔").green().bold()
        );
        return;
    }

    println!();
    println!("  {}", style("Missing dependencies:").yellow().bold());
    for dep in report.missing_required() {
        print_missing_dep(dep);
    }
}

/// Render one missing dependency: its id, ordered install commands, and docs.
fn print_missing_dep(dep: &crate::doctor::DepStatus) {
    println!();
    println!(
        "  {} {} (required)",
        style("✘").red().bold(),
        style(&dep.id).bold()
    );
    for step in &dep.plan {
        println!("      {}", style(&step.command).cyan());
    }
    if dep.plan.is_empty() {
        println!(
            "      {}",
            style("(no install method detected for this system)").dim()
        );
    }
    if let Some(docs) = &dep.docs {
        println!(
            "      {} {}",
            style("Docs:").dim(),
            style(docs).underlined()
        );
    }
}

/// Sorted installer keys detected on this host.
fn detected_installers(env: &Environment) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Installer::ALL
        .iter()
        .filter(|i| env.installers.contains(i))
        .map(|i| i.key())
        .collect();
    keys.sort_unstable();
    keys
}

/// Print the standalone `doctor` report: environment + detected components +
/// dependency status.
pub fn print_doctor(
    scan: &ScanResult,
    report: &Report,
    env: &Environment,
    registry: &Registry,
    format: Format,
) {
    let installers = detected_installers(env);

    if format == Format::Json {
        emit_json_ok(json!({
            "all_required_present": report.all_required_present,
            "environment": { "os": env.os, "installers": installers },
            "components": scan.components,
            "unrecognized": scan.unrecognized,
            "deps": report.deps,
        }));
        return;
    }

    println!();
    println!("  {}", style("Dependency check").bold().underlined());

    // Environment.
    println!();
    let installer_list = if installers.is_empty() {
        "none detected".to_string()
    } else {
        installers.join(", ")
    };
    println!(
        "  {} {:?}   {} {}",
        style("OS:").dim(),
        env.os,
        style("Installers:").dim(),
        installer_list
    );

    // Detected components. The mark reflects whether the tool's required
    // dependencies are present, not merely that the component was detected.
    let present: std::collections::HashSet<&str> = report
        .deps
        .iter()
        .filter(|d| d.present)
        .map(|d| d.id.as_str())
        .collect();

    println!();
    if scan.components.is_empty() && scan.unrecognized.is_empty() {
        println!(
            "  {}",
            style("No generated components detected in this directory.").dim()
        );
    } else {
        for comp in &scan.components {
            let tool = registry.get(&comp.tool_id);
            let name = tool.map(|t| t.name.as_str()).unwrap_or(&comp.tool_id);
            let missing: Vec<&str> = tool
                .map(|t| {
                    t.system_deps
                        .iter()
                        .map(|d| d.as_str())
                        .filter(|d| !present.contains(d))
                        .collect()
                })
                .unwrap_or_default();

            if missing.is_empty() {
                println!(
                    "  {} {}: {}",
                    style("✔").green().bold(),
                    comp.kind,
                    style(name).cyan()
                );
            } else {
                println!(
                    "  {} {}: {} {}",
                    style("✘").red().bold(),
                    comp.kind,
                    style(name).cyan(),
                    style(format!("(missing: {})", missing.join(", "))).yellow()
                );
            }
        }
        for un in &scan.unrecognized {
            println!(
                "  {} {}/ — unrecognized (renamed or modified?)",
                style("?").yellow().bold(),
                un.dir
            );
        }
    }

    // Dependency status.
    println!();
    for dep in &report.deps {
        if dep.present {
            println!("  {} {}", style("✔").green().bold(), dep.id);
        }
    }
    print_dep_advice(report);
    println!();
}

/// Print the registry (roles + tools) for `cardano-init list`: human-readable
/// by default, or the TECH_SPEC §8 JSON payload with `--format json`.
pub fn print_list(registry: &Registry, format: Format) {
    use crate::registry::view;

    if format == Format::Json {
        emit_json_ok(json!({
            "roles": view::role_views(),
            "tools": view::tool_views(registry),
        }));
        return;
    }

    println!();
    println!("  {}", style("Roles").bold().underlined());
    println!();
    for role in view::role_views() {
        let multi = if role.multiple { "  (multiple)" } else { "" };
        println!(
            "  {:<16}{:<18}{}{}",
            style(role.id).cyan(),
            role.display,
            style(format!("dir: {}", role.dir)).dim(),
            style(multi).dim()
        );
    }

    println!();
    println!("  {}", style("Tools").bold().underlined());
    // Reuse the same per-tool block as `--help` so the two can't drift; sort by
    // id to match the JSON ordering.
    let mut tools: Vec<&crate::registry::types::ToolDef> = registry.all_tools().iter().collect();
    tools.sort_by(|a, b| a.id.cmp(&b.id));
    let mut block = String::new();
    for tool in tools {
        block.push('\n');
        super::format_tool(&mut block, tool);
    }
    print!("{block}");
    println!();
}

/// Truncate a tool description to the first sentence for use in prompts.
pub fn first_sentence(desc: &str) -> &str {
    // Find the first period followed by whitespace or end-of-string
    if let Some(pos) = desc.find(". ") {
        &desc[..=pos]
    } else if let Some(pos) = desc.find(".\n") {
        &desc[..=pos]
    } else if desc.ends_with('.') {
        desc
    } else {
        // No sentence boundary — take first 80 chars
        let end = desc
            .char_indices()
            .nth(80)
            .map(|(i, _)| i)
            .unwrap_or(desc.len());
        &desc[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sentence_with_period_space() {
        assert_eq!(
            first_sentence("Hello world. More text here."),
            "Hello world."
        );
    }

    #[test]
    fn first_sentence_no_period() {
        assert_eq!(first_sentence("No period here"), "No period here");
    }

    #[test]
    fn first_sentence_ends_with_period() {
        assert_eq!(first_sentence("Ends with period."), "Ends with period.");
    }

    use crate::registry::types::{Network, RoleAssignment};

    fn selection_with(assignments: Vec<RoleAssignment>) -> Selection {
        Selection {
            project_name: "demo".to_string(),
            assignments,
            network: Network::Preview,
            nix: false,
        }
    }

    #[test]
    fn experimental_tag_only_for_experimental_tools() {
        let reg = Registry::load().unwrap();
        assert_eq!(
            experimental_tag(reg.get("blaster").unwrap()),
            " [experimental]"
        );
        assert_eq!(experimental_tag(reg.get("aiken").unwrap()), "");
    }

    #[test]
    fn components_json_marks_experimental_component() {
        let reg = Registry::load().unwrap();
        let sel = selection_with(vec![
            RoleAssignment {
                role: Role::OnChain,
                tool_id: "aiken".to_string(),
            },
            RoleAssignment {
                role: Role::FormalMethods,
                tool_id: "blaster".to_string(),
            },
        ]);
        let json = components_json(&sel, &reg);
        let items = json.as_array().unwrap();
        let aiken = items.iter().find(|c| c["tool"] == "aiken").unwrap();
        let blaster = items.iter().find(|c| c["tool"] == "blaster").unwrap();
        assert_eq!(aiken["experimental"], serde_json::json!(false));
        assert_eq!(blaster["experimental"], serde_json::json!(true));
    }

    #[test]
    fn selected_experimental_tools_dedups_and_filters() {
        let reg = Registry::load().unwrap();
        // No experimental tool selected → empty.
        let none = selection_with(vec![RoleAssignment {
            role: Role::OnChain,
            tool_id: "aiken".to_string(),
        }]);
        assert!(selected_experimental_tools(&none, &reg).is_empty());

        // Blaster selected → reported once.
        let some = selection_with(vec![RoleAssignment {
            role: Role::FormalMethods,
            tool_id: "blaster".to_string(),
        }]);
        let experimental = selected_experimental_tools(&some, &reg);
        assert_eq!(experimental.len(), 1);
        assert_eq!(experimental[0].id, "blaster");
    }
}
