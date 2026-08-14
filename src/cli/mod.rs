pub mod interactive;
pub mod oneshot;
pub mod output;

use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::registry::loader::{Registry, RegistryError};
use crate::registry::types::{Role, ToolDef};
use crate::scaffold::ScaffoldError;

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

/// Output format. `json` implies non-interactive: it never prompts; if required
/// input is missing it errors instead (TECH_SPEC §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    Human,
    Json,
}

/// Scaffold a new Cardano protocol project.
#[derive(Parser, Debug)]
#[command(name = "cardano-init", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Output format (global): human-readable text or machine-readable JSON
    #[arg(long, value_enum, global = true, default_value = "human")]
    pub format: Format,

    #[command(flatten)]
    pub init: InitArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the web-based project builder on localhost
    Web {
        /// Port to listen on
        #[arg(long, default_value_t = 3000)]
        port: u16,
    },

    /// Check that the dependencies this project needs are installed, and
    /// advise how to install any that are missing
    Doctor,

    /// List the available roles and tools (use --format json for agents)
    List,
}

/// Arguments for the default init mode (interactive or one-shot).
#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Project name (required in one-shot mode)
    #[arg(long)]
    pub name: Option<String>,

    /// On-chain tool (e.g., aiken, scalus)
    #[arg(long, value_name = "TOOL_ID")]
    pub on_chain: Option<String>,

    /// Off-chain tool (e.g., meshjs, scalus)
    #[arg(long, value_name = "TOOL_ID")]
    pub off_chain: Option<String>,

    /// Fullstack tool for both on-chain and off-chain, as one `protocol/`
    /// component (e.g. scalus). Sugar for --on-chain X --off-chain X; the tool
    /// must declare a [fullstack] template. Not combinable with --on-chain/--off-chain.
    #[arg(long, value_name = "TOOL_ID")]
    pub fullstack: Option<String>,

    /// Infrastructure tool (repeatable: --infra kupo --infra ogmios)
    #[arg(long, value_name = "TOOL_ID")]
    pub infra: Vec<String>,

    /// Devnet tool (e.g., yaci)
    #[arg(long, value_name = "TOOL_ID")]
    pub devnet: Option<String>,

    /// Formal methods tool (e.g., blaster)
    #[arg(long, value_name = "TOOL_ID")]
    pub formal_methods: Option<String>,

    /// Target network
    #[arg(long, default_value = "preview")]
    pub network: String,

    /// Generate Nix flake for dependency management
    #[arg(long)]
    pub nix: bool,

    /// Opt in to experimental tools (not yet build-green). Required to select an
    /// experimental tool in one-shot/JSON mode; pre-acknowledges the interactive
    /// confirm.
    #[arg(long)]
    pub allow_experimental: bool,

    /// Show what would be generated without writing to disk
    #[arg(long)]
    pub dry_run: bool,

    /// Scaffold a combination the compatibility check flags as incompatible
    /// (e.g. an off-chain tool and a devnet that can't talk). Downgrades the
    /// stop-generation error to a warning.
    #[arg(long)]
    pub ignore_warning: bool,
}

impl InitArgs {
    /// Returns true if any one-shot flags were provided.
    fn has_oneshot_flags(&self) -> bool {
        self.on_chain.is_some()
            || self.off_chain.is_some()
            || self.fullstack.is_some()
            || !self.infra.is_empty()
            || self.devnet.is_some()
            || self.formal_methods.is_some()
            || self.nix
            || self.dry_run
            || self.network != "preview"
    }
}

// ---------------------------------------------------------------------------
// CLI errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Registry(#[from] RegistryError),

    #[error("{0}")]
    Scaffold(#[from] ScaffoldError),

    #[error("{0}")]
    Web(#[from] crate::web::WebError),

    #[error("{0}")]
    Catalog(#[from] crate::doctor::catalog::CatalogError),

    #[error("directory '{}' already exists. Refusing to overwrite", path)]
    DirectoryExists { path: String },

    #[error("unknown tool '{}' for role {}", tool_id, role)]
    UnknownTool {
        tool_id: String,
        role: String,
        valid_tools: Vec<String>,
    },

    #[error("tool '{}' does not support role '{}'", tool_id, role)]
    ToolRoleMismatch {
        tool_id: String,
        role: String,
        valid_roles: Vec<String>,
    },

    #[error("no roles selected. At least one role must be provided")]
    NoRolesSelected,

    #[error(
        "{} is experimental — it may be unstable or incomplete, so it's opt-in (expect rough edges and breaking changes).\n\n  To scaffold it anyway, re-run the same command with --allow-experimental, e.g.:\n\n    cardano-init --name <project> {} --allow-experimental",
        labels.join(", "),
        example_flags.join(" ")
    )]
    ExperimentalNotAllowed {
        /// Tool ids (machine-facing `context`).
        tools: Vec<String>,
        /// Human labels, `Name (id)`, for the message.
        labels: Vec<String>,
        /// The role flags that selected the experimental tool(s), e.g.
        /// `["--formal-methods", "blaster"]`, so the example is copy-pasteable.
        example_flags: Vec<String>,
    },

    #[error(
        "tool '{}' does not support fullstack (no [fullstack] template)",
        tool_id
    )]
    FullstackUnsupported {
        tool_id: String,
        valid_tools: Vec<String>,
    },

    #[error(
        "--fullstack cannot be combined with --on-chain or --off-chain\n\n  --fullstack X already fills both roles; use it alone."
    )]
    FullstackConflict,

    // Boxed to keep `CliError` small (this payload carries several strings).
    #[error("{0}")]
    IncompatibleTools(Box<IncompatibleToolsError>),

    #[error("invalid network '{}'. Expected preview, preprod, or mainnet", value)]
    InvalidNetwork { value: String },

    #[error("invalid project name '{}' — {}", name, reason)]
    InvalidProjectName { name: String, reason: String },

    #[error(
        "--name is required when using one-shot flags (--on-chain, --off-chain, etc.)\n\n  Run without flags for interactive mode, or provide --name:\n\n    cardano-init --name my-protocol --on-chain aiken"
    )]
    NameRequired,

    #[error("user aborted")]
    Aborted,

    #[error("prompt error: {0}")]
    Prompt(#[from] dialoguer::Error),
}

/// Payload for [`CliError::IncompatibleTools`] — boxed in the variant so
/// `CliError` stays small (an off-chain ↔ provider mismatch carries several
/// strings for both the human message and the JSON context).
#[derive(Debug)]
pub struct IncompatibleToolsError {
    /// Human label `Name (id)` for the off-chain tool.
    pub off_chain: String,
    /// Human labels `Name (id)` for the offending provider(s), comma-joined.
    pub providers: String,
    /// Why the pair can't talk (seam mismatch / self-hosting).
    pub reason: String,
    /// Pre-formatted multi-line remedy (which providers / off-chain tools fit).
    pub help: String,
    /// Machine-facing ids `[off_chain_id, provider_ids…]` (JSON context).
    pub ids: Vec<String>,
    /// Provider ids compatible with the chosen off-chain tool (JSON context).
    pub compatible_providers: Vec<String>,
    /// Off-chain ids compatible with the chosen providers (JSON context).
    pub compatible_off_chain: Vec<String>,
}

impl std::fmt::Display for IncompatibleToolsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} can't be paired with {} — {}.\n\n{}\n\n  To scaffold this combination anyway, re-run with --ignore-warning.",
            self.off_chain, self.providers, self.reason, self.help
        )
    }
}

impl CliError {
    /// Process exit-code category (TECH_SPEC §2.3):
    /// - `2` — usage / validation errors (bad or missing input);
    /// - `1` — runtime errors (I/O, registry/render failure, web bind, …);
    /// - `0` — interactive abort by user choice (not an error).
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::UnknownTool { .. }
            | CliError::ToolRoleMismatch { .. }
            | CliError::NoRolesSelected
            | CliError::ExperimentalNotAllowed { .. }
            | CliError::FullstackUnsupported { .. }
            | CliError::FullstackConflict
            | CliError::IncompatibleTools(_)
            | CliError::InvalidNetwork { .. }
            | CliError::InvalidProjectName { .. }
            | CliError::NameRequired => 2,

            CliError::Aborted => 0,

            CliError::Registry(_)
            | CliError::Scaffold(_)
            | CliError::Web(_)
            | CliError::Catalog(_)
            | CliError::DirectoryExists { .. }
            | CliError::Prompt(_) => 1,
        }
    }

    /// Stable, machine-readable error code (TECH_SPEC §2.5). Part of the JSON
    /// contract; never changes for a given error kind.
    pub fn code(&self) -> &'static str {
        match self {
            CliError::Registry(_) | CliError::Catalog(_) => "registry_load",
            CliError::Scaffold(_) => "scaffold_error",
            CliError::Web(_) => "web_bind",
            CliError::DirectoryExists { .. } => "dir_exists",
            CliError::UnknownTool { .. } => "unknown_tool",
            CliError::ToolRoleMismatch { .. } => "tool_role_mismatch",
            CliError::NoRolesSelected => "no_roles_selected",
            CliError::ExperimentalNotAllowed { .. } => "experimental_not_allowed",
            CliError::FullstackUnsupported { .. } => "fullstack_unsupported",
            CliError::FullstackConflict => "fullstack_conflict",
            CliError::IncompatibleTools(_) => "incompatible_tools",
            CliError::InvalidNetwork { .. } => "invalid_network",
            CliError::InvalidProjectName { .. } => "invalid_project_name",
            CliError::NameRequired => "name_required",
            CliError::Aborted => "aborted",
            CliError::Prompt(_) => "prompt_error",
        }
    }

    /// Structured, agent-facing context: the offending input plus valid
    /// alternatives where applicable (TECH_SPEC §2.5, PRD FR-15).
    pub fn context(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            CliError::Registry(e) => json!({ "detail": e.to_string() }),
            CliError::Catalog(e) => json!({ "detail": e.to_string() }),
            CliError::Scaffold(e) => json!({ "detail": e.to_string() }),
            CliError::Web(crate::web::WebError::Bind { port, source }) => {
                json!({ "port": port, "detail": source.to_string() })
            }
            CliError::DirectoryExists { path } => json!({ "path": path }),
            CliError::UnknownTool {
                tool_id,
                role,
                valid_tools,
            } => json!({ "tool_id": tool_id, "role": role, "valid_tools": valid_tools }),
            CliError::ToolRoleMismatch {
                tool_id,
                role,
                valid_roles,
            } => json!({ "tool_id": tool_id, "role": role, "valid_roles": valid_roles }),
            CliError::FullstackUnsupported {
                tool_id,
                valid_tools,
            } => json!({ "tool_id": tool_id, "valid_tools": valid_tools }),
            CliError::InvalidNetwork { value } => {
                json!({ "value": value, "expected": ["preview", "preprod", "mainnet"] })
            }
            CliError::InvalidProjectName { name, reason } => {
                json!({ "name": name, "reason": reason })
            }
            CliError::ExperimentalNotAllowed { tools, .. } => {
                json!({ "tools": tools, "remedy": "--allow-experimental" })
            }
            CliError::IncompatibleTools(e) => json!({
                "tools": e.ids,
                "reason": e.reason,
                "compatible_providers": e.compatible_providers,
                "compatible_off_chain": e.compatible_off_chain,
                "remedy": "--ignore-warning",
            }),
            CliError::NoRolesSelected
            | CliError::FullstackConflict
            | CliError::NameRequired
            | CliError::Aborted
            | CliError::Prompt(_) => json!({}),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool catalog for --help
// ---------------------------------------------------------------------------

/// Build the "Available tools" section appended to --help output.
fn build_tool_catalog(registry: &Registry) -> String {
    use std::fmt::Write;

    let mut out = String::from("Available tools:\n");

    for tool in registry.all_tools() {
        out.push('\n');
        format_tool(&mut out, tool);
    }

    let _ = writeln!(out, "\nExamples:");
    let _ = writeln!(
        out,
        "  cardano-init                                        # interactive mode"
    );
    let _ = writeln!(
        out,
        "  cardano-init --name my-app --on-chain aiken         # one-shot, single role"
    );
    let _ = writeln!(
        out,
        "  cardano-init --name my-app --on-chain aiken --off-chain meshjs --nix"
    );
    let _ = writeln!(
        out,
        "  cardano-init web                                    # web-based builder"
    );

    out
}

pub(super) fn format_tool(out: &mut String, tool: &ToolDef) {
    use std::fmt::Write;

    let mut roles: Vec<&str> = tool.roles.keys().map(|r| r.as_kebab()).collect();
    roles.sort();
    let experimental_tag = if tool.experimental {
        "  [experimental]"
    } else {
        ""
    };
    let _ = writeln!(out, "  {} ({}){}", tool.name, tool.id, experimental_tag);
    if tool.experimental {
        let _ = writeln!(
            out,
            "    Status:    experimental — may be unstable or incomplete; needs --allow-experimental"
        );
    }
    let _ = writeln!(out, "    Roles:     {}", roles.join(", "));
    let _ = writeln!(out, "    Languages: {}", tool.languages.join(", "));
    let _ = writeln!(out, "    Website:   {}", tool.website);

    // Wrap description to ~72 chars with 4-space indent
    let _ = write!(out, "    ");
    let mut col = 4;
    for word in tool.description.split_whitespace() {
        if col + word.len() + 1 > 76 && col > 4 {
            let _ = write!(out, "\n    ");
            col = 4;
        }
        if col > 4 {
            let _ = write!(out, " ");
            col += 1;
        }
        let _ = write!(out, "{word}");
        col += word.len();
    }
    let _ = writeln!(out);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Main CLI entry point. Parse args, dispatch, and present the result (or a
/// machine-readable error). Returns the process exit code.
pub fn run() -> i32 {
    let registry = match Registry::load() {
        Ok(r) => r,
        Err(e) => {
            // Registry load happens before we know the requested format; this
            // is a packaging bug (embedded data), so default to human output.
            let err = CliError::from(e);
            output::print_error(&err, Format::Human);
            return err.exit_code();
        }
    };

    // Build clap command with dynamic after_help containing tool catalog
    let catalog = build_tool_catalog(&registry);
    let cmd = Cli::command().after_help(catalog);
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("clap already validated");
    let format = cli.format;

    let result = match cli.command {
        Some(Command::Web { port }) => crate::web::serve(&registry, port).map_err(CliError::from),
        Some(Command::Doctor) => run_doctor(&registry, format),
        Some(Command::List) => {
            output::print_list(&registry, format);
            Ok(())
        }
        None => run_init(cli.init, &registry, format),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            output::print_error(&e, format);
            e.exit_code()
        }
    }
}

/// The required dependencies for a set of tool ids: the base dep `just`, plus
/// the `system_deps` of each tool (deduped/sorted later by the resolver).
///
/// The aggregated infra component is reported by the scan under the synthetic
/// `INFRA_DRIVER_ID`; it contributes the union of every infra tool's
/// `system_deps` (data-driven from the registry — `{docker, cardano-up}`).
fn required_deps<'a>(tool_ids: impl Iterator<Item = &'a str>, registry: &Registry) -> Vec<String> {
    let mut deps = vec![crate::doctor::BASE_DEP.to_string()];
    for id in tool_ids {
        if id == crate::doctor::INFRA_DRIVER_ID {
            for tool in registry.tools_for_role(crate::registry::types::Role::Infrastructure) {
                deps.extend(tool.system_deps.iter().cloned());
            }
        } else if let Some(tool) = registry.get(id) {
            deps.extend(tool.system_deps.iter().cloned());
        }
    }
    deps
}

/// Turn a detected [`Incompatibility`](crate::registry::compat::Incompatibility)
/// into a stop-generation `CliError`, pre-formatting the "which tools do fit"
/// remedy (with the self-hosting case — no compatible provider — phrased as
/// "omit the provider").
fn incompatible_tools_error(inc: crate::registry::compat::Incompatibility) -> CliError {
    let provider_labels: Vec<String> = inc
        .providers
        .iter()
        .map(|p| format!("{} ({})", p.name, p.id))
        .collect();
    let providers_text = provider_labels.join(", ");

    let providers_line = if !inc.compatible_providers.is_empty() {
        format!(
            "  Providers that work with {}: {}",
            inc.off_chain_name,
            inc.compatible_providers.join(", ")
        )
    } else if inc.self_hosted {
        format!(
            "  {} provides its own devnet — omit the provider selection.",
            inc.off_chain_name
        )
    } else {
        format!(
            "  No bundled provider serves {} — point it at a public provider (see its README).",
            inc.off_chain_name
        )
    };
    let off_chain_line = if inc.compatible_off_chain.is_empty() {
        String::new()
    } else {
        format!(
            "\n  Off-chain tools that work with {}: {}",
            providers_text,
            inc.compatible_off_chain.join(", ")
        )
    };

    let mut ids = vec![inc.off_chain_id.clone()];
    ids.extend(inc.providers.iter().map(|p| p.id.clone()));

    CliError::IncompatibleTools(Box::new(IncompatibleToolsError {
        off_chain: format!("{} ({})", inc.off_chain_name, inc.off_chain_id),
        providers: providers_text,
        reason: inc.reason,
        help: format!("{providers_line}{off_chain_line}"),
        ids,
        compatible_providers: inc.compatible_providers,
        compatible_off_chain: inc.compatible_off_chain,
    }))
}

/// Run the standalone `doctor`: scan the current directory for generated
/// components, then report dependency status + install plans.
fn run_doctor(registry: &Registry, format: Format) -> Result<(), CliError> {
    use crate::doctor::{self, catalog::DepCatalog, probe};

    let catalog = DepCatalog::load()?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let scan = probe::scan_project(&cwd, registry);

    let required = required_deps(scan.components.iter().map(|c| c.tool_id.as_str()), registry);
    let env = probe::detect_environment(&catalog);
    let report = doctor::resolve_all(&required, &catalog, &env);

    output::print_doctor(&scan, &report, &env, registry, format);
    Ok(())
}

/// The one-shot flag that assigns a tool to a given role, e.g. `--on-chain`.
/// Used to build a copy-pasteable example command in error messages.
fn role_flag(role: Role) -> &'static str {
    match role {
        Role::OnChain => "--on-chain",
        Role::OffChain => "--off-chain",
        Role::Infrastructure => "--infra",
        Role::Devnet => "--devnet",
        Role::FormalMethods => "--formal-methods",
    }
}

/// Run the default init mode (interactive or one-shot).
fn run_init(args: InitArgs, registry: &Registry, format: Format) -> Result<(), CliError> {
    // `--name` is required for one-shot flags, and always in JSON mode (which
    // is non-interactive and must never prompt — TECH_SPEC §2.1).
    if args.name.is_none() && (args.has_oneshot_flags() || format == Format::Json) {
        return Err(CliError::NameRequired);
    }

    // `--fullstack X` is sugar for `--on-chain X --off-chain X`; combining it with
    // an explicit on-chain/off-chain flag is ambiguous (TECH_SPEC §2.2).
    if args.fullstack.is_some() && (args.on_chain.is_some() || args.off_chain.is_some()) {
        return Err(CliError::FullstackConflict);
    }

    // Decide mode: one-shot if --name provided, interactive otherwise
    let selection = if let Some(ref name) = args.name {
        let selection = oneshot::build_selection(
            name,
            args.on_chain.as_deref(),
            args.off_chain.as_deref(),
            args.fullstack.as_deref(),
            &args.infra,
            args.devnet.as_deref(),
            args.formal_methods.as_deref(),
            &args.network,
            args.nix,
            registry,
        )?;
        // Experimental gate (one-shot / JSON): selecting a not-yet-build-green
        // tool requires explicit opt-in. Non-interactive can't prompt, so this
        // is a hard usage error unless --allow-experimental was passed.
        if !args.allow_experimental {
            let experimental = output::selected_experimental_tools(&selection, registry);
            if !experimental.is_empty() {
                let tools: Vec<String> = experimental.iter().map(|t| t.id.clone()).collect();
                let labels: Vec<String> = experimental
                    .iter()
                    .map(|t| format!("{} ({})", t.name, t.id))
                    .collect();
                // The exact role flags that pulled in the experimental tool(s),
                // so the suggested command is copy-pasteable.
                let example_flags: Vec<String> = selection
                    .assignments
                    .iter()
                    .filter(|a| tools.iter().any(|id| id == &a.tool_id))
                    .flat_map(|a| [role_flag(a.role).to_string(), a.tool_id.clone()])
                    .collect();
                return Err(CliError::ExperimentalNotAllowed {
                    tools,
                    labels,
                    example_flags,
                });
            }
        }
        selection
    } else {
        interactive::run_interactive(registry, args.allow_experimental, args.ignore_warning)?
    };

    // Off-chain ↔ devnet compatibility gate. Interactive mode already filters
    // incompatible options at selection time, so this primarily guards one-shot;
    // with --ignore-warning it degrades to a warning and proceeds anyway.
    if let Some(inc) = crate::registry::compat::check(&selection.assignments, registry) {
        if args.ignore_warning {
            if format == Format::Human {
                output::print_incompatibility_warning(&inc);
            }
        } else {
            return Err(incompatible_tools_error(inc));
        }
    }

    let root = PathBuf::from(&selection.project_name);

    // Safety: refuse to write into an existing, non-empty directory (never
    // overwrite user files). A missing or empty target dir is fine (§6.4).
    if root.exists()
        && std::fs::read_dir(&root)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(true)
    {
        return Err(CliError::DirectoryExists {
            path: selection.project_name.clone(),
        });
    }

    if args.dry_run {
        let plan = crate::scaffold::dry_run(&selection, registry)?;
        output::print_dry_run(&selection, registry, &plan, format);
        return Ok(());
    }

    if format == Format::Human {
        output::print_summary(&selection, registry);
    }
    crate::scaffold::scaffold(&selection, registry, &root)?;

    // Check-and-advise: resolve the deps this selection needs (TECH_SPEC §9).
    let report = resolve_selection_deps(&selection, registry)?;
    output::print_success(&selection, registry, &report, format);

    Ok(())
}

/// Resolve the dependency report for a generated selection (check-and-advise).
fn resolve_selection_deps(
    selection: &crate::registry::types::Selection,
    registry: &Registry,
) -> Result<crate::doctor::Report, CliError> {
    use crate::doctor::{self, catalog::DepCatalog, probe};

    let catalog = DepCatalog::load()?;
    let required = required_deps(
        selection.assignments.iter().map(|a| a.tool_id.as_str()),
        registry,
    );
    let env = probe::detect_environment(&catalog);
    Ok(doctor::resolve_all(&required, &catalog, &env))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_error_code_and_context() {
        let registry = Registry::load().unwrap();
        // `bogus` is not a tool; one-shot validation should surface it with the
        // stable code + the valid alternatives for the role.
        let err = oneshot::build_selection(
            "demo",
            Some("bogus"),
            None,
            None,
            &[],
            None,
            None,
            "preview",
            false,
            &registry,
        )
        .unwrap_err();

        assert_eq!(err.code(), "unknown_tool");
        let ctx = err.context();
        assert_eq!(ctx["tool_id"], "bogus");
        assert_eq!(ctx["role"], "On-chain");
        // on-chain is fillable by aiken + plinth + scalus.
        let valid: Vec<&str> = ctx["valid_tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(valid, vec!["aiken", "plinth", "scalus"]);
    }

    #[test]
    fn tool_role_mismatch_lists_valid_roles() {
        let registry = Registry::load().unwrap();
        // aiken is on-chain only; asking it to fill off-chain is a mismatch.
        let err = oneshot::build_selection(
            "demo",
            None,
            Some("aiken"),
            None,
            &[],
            None,
            None,
            "preview",
            false,
            &registry,
        )
        .unwrap_err();
        assert_eq!(err.code(), "tool_role_mismatch");
        let ctx = err.context();
        assert_eq!(ctx["valid_roles"], serde_json::json!(["on-chain"]));
    }

    #[test]
    fn invalid_network_context_lists_expected() {
        let registry = Registry::load().unwrap();
        let err = oneshot::build_selection(
            "demo",
            Some("aiken"),
            None,
            None,
            &[],
            None,
            None,
            "badnet",
            false,
            &registry,
        )
        .unwrap_err();
        assert_eq!(err.code(), "invalid_network");
        assert_eq!(
            err.context()["expected"],
            serde_json::json!(["preview", "preprod", "mainnet"])
        );
    }

    #[test]
    fn fullstack_conflict_with_explicit_role_flag() {
        // --fullstack combined with --on-chain is a usage error; caught in
        // run_init before any generation happens.
        let registry = Registry::load().unwrap();
        let args = InitArgs {
            name: Some("demo".to_string()),
            on_chain: Some("aiken".to_string()),
            off_chain: None,
            fullstack: Some("scalus".to_string()),
            infra: vec![],
            devnet: None,
            formal_methods: None,
            network: "preview".to_string(),
            nix: false,
            allow_experimental: false,
            dry_run: true,
            ignore_warning: false,
        };
        let err = run_init(args, &registry, Format::Json).unwrap_err();
        assert_eq!(err.code(), "fullstack_conflict");
        assert_eq!(err.exit_code(), 2);
    }

    /// Build InitArgs for a one-shot run with just the fields a test varies.
    fn init_args(formal_methods: Option<&str>, allow_experimental: bool) -> InitArgs {
        InitArgs {
            name: Some("exp-gate-demo".to_string()),
            on_chain: Some("aiken".to_string()),
            off_chain: None,
            fullstack: None,
            infra: vec![],
            devnet: None,
            formal_methods: formal_methods.map(str::to_string),
            network: "preview".to_string(),
            nix: false,
            allow_experimental,
            dry_run: true,
            ignore_warning: false,
        }
    }

    #[test]
    fn experimental_tool_gated_without_flag() {
        // One-shot selecting blaster (experimental) without --allow-experimental
        // is a usage error before any generation happens.
        let registry = Registry::load().unwrap();
        let err = run_init(init_args(Some("blaster"), false), &registry, Format::Json).unwrap_err();
        assert_eq!(err.code(), "experimental_not_allowed");
        assert_eq!(err.exit_code(), 2);
        let ctx = err.context();
        let tools: Vec<&str> = ctx["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(tools, vec!["blaster"]);
        assert_eq!(ctx["remedy"], "--allow-experimental");

        // The human message names the tool (name + id) and shows a copy-pasteable
        // opt-in command with the exact role flag.
        let msg = err.to_string();
        assert!(msg.contains("Blaster (blaster)"), "message: {msg}");
        assert!(
            msg.contains("--formal-methods blaster --allow-experimental"),
            "message: {msg}"
        );
    }

    #[test]
    fn experimental_tool_allowed_with_flag() {
        // With --allow-experimental the same selection passes the gate (dry-run
        // → Ok, no disk write).
        let registry = Registry::load().unwrap();
        let res = run_init(init_args(Some("blaster"), true), &registry, Format::Json);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
    }

    #[test]
    fn non_experimental_selection_needs_no_flag() {
        // A selection with no experimental tool is unaffected by the gate.
        let registry = Registry::load().unwrap();
        let res = run_init(init_args(None, false), &registry, Format::Json);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
    }

    /// InitArgs for a one-shot off-chain + infra run whose seams don't overlap
    /// (Evolution speaks Blockfrost/Kupmios; Dolos serves only UTxORPC).
    /// `ignore_warning` toggles the compat gate.
    fn incompatible_provider_args(ignore_warning: bool) -> InitArgs {
        InitArgs {
            name: Some("compat-demo".to_string()),
            on_chain: None,
            off_chain: Some("evolution".to_string()),
            fullstack: None,
            infra: vec!["dolos".to_string()],
            devnet: None,
            formal_methods: None,
            network: "preview".to_string(),
            nix: false,
            allow_experimental: false,
            dry_run: true,
            ignore_warning,
        }
    }

    #[test]
    fn incompatible_offchain_provider_is_gated() {
        // Evolution (Blockfrost/Kupmios) + Dolos (UTxORPC) share no seam, so
        // generation stops before any disk write.
        let registry = Registry::load().unwrap();
        let err = run_init(incompatible_provider_args(false), &registry, Format::Json).unwrap_err();
        assert_eq!(err.code(), "incompatible_tools");
        assert_eq!(err.exit_code(), 2);
        let ctx = err.context();
        assert_eq!(ctx["remedy"], "--ignore-warning");
        let tools: Vec<&str> = ctx["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(tools, vec!["evolution", "dolos"]);
    }

    #[test]
    fn incompatible_offchain_provider_allowed_with_ignore_warning() {
        // --ignore-warning downgrades the stop to a warning and proceeds.
        let registry = Registry::load().unwrap();
        let res = run_init(incompatible_provider_args(true), &registry, Format::Json);
        assert!(res.is_ok(), "expected Ok, got {res:?}");
    }

    #[test]
    fn required_deps_unions_just_with_tool_deps() {
        let registry = Registry::load().unwrap();
        // aiken → ["aiken"], meshjs → ["node"]; plus the base dep "just".
        let deps = required_deps(["aiken", "meshjs"].into_iter(), &registry);
        assert!(deps.contains(&"just".to_string()));
        assert!(deps.contains(&"aiken".to_string()));
        assert!(deps.contains(&"node".to_string()));
    }

    #[test]
    fn required_deps_resolves_infra_driver_to_union() {
        let registry = Registry::load().unwrap();
        // The aggregated infra component (reported under INFRA_DRIVER_ID by the
        // scan) contributes the union of infra tools' system_deps.
        let deps = required_deps([crate::doctor::INFRA_DRIVER_ID].into_iter(), &registry);
        assert!(deps.contains(&"just".to_string()));
        assert!(deps.contains(&"docker".to_string()));
        assert!(deps.contains(&"cardano-up".to_string()));
    }
}
