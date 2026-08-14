//! Off-chain ↔ provider compatibility, decided purely from registry `[compat]`
//! data — no per-tool special-casing.
//!
//! An off-chain tool reaches a local chain over one or more "seams" (wire
//! protocols): Tx3 speaks TRP, Evolution speaks Blockfrost/Kupmios, Mesh speaks
//! Blockfrost/UTxORPC/Ogmios. A **provider** — the selected devnet and/or the
//! selected infra tools — exposes some set of seams (Yaci serves Blockfrost,
//! Dolos serves UTxORPC, Kupo serves Kupo, …). The off-chain tool is satisfied
//! when at least one selected provider serves a seam it consumes; if the
//! providers between them serve none of them, the pairing can't work.
//!
//! Two special cases:
//!   - A tool that bundles its own devnet (`self_contained_devnet`, e.g. Tx3)
//!     needs no devnet role at all, so pairing it with one is redundant.
//!   - Providers (and off-chain tools) that declare no seams impose no
//!     constraint — supplementary infra (a bare node, a submit API) and
//!     not-yet-annotated tools never fabricate a conflict.
//!
//! This module is pure logic over registry data (no CLI dependency), matching
//! the `registry`/`scaffold` purity invariant. The CLI turns an
//! [`Incompatibility`] into a stop-generation error (or, with `--ignore-warning`,
//! a warning) and drives interactive filtering.

use std::collections::HashSet;

use super::loader::Registry;
use super::types::{Role, RoleAssignment, Seam, ToolDef};

/// A provider tool (devnet or infra) referenced in a compatibility report.
#[derive(Debug, Clone)]
pub struct ProviderRef {
    pub id: String,
    pub name: String,
}

/// A detected incompatibility between the selected off-chain tool and the
/// selected providers (devnet + infra).
#[derive(Debug, Clone)]
pub struct Incompatibility {
    pub off_chain_id: String,
    pub off_chain_name: String,
    /// Human-readable explanation of why the pairing can't work.
    pub reason: String,
    /// Whether the off-chain tool bundles its own devnet (shapes the remedy).
    pub self_hosted: bool,
    /// The selected providers that can't serve the off-chain tool.
    pub providers: Vec<ProviderRef>,
    /// Provider tool ids (devnet/infra) in the registry that *would* serve the
    /// off-chain tool. Empty when it self-hosts (then no provider is needed).
    pub compatible_providers: Vec<String>,
    /// Off-chain tool ids that *would* work with the selected providers.
    pub compatible_off_chain: Vec<String>,
}

/// Check a full selection for an off-chain ↔ provider incompatibility. Returns
/// `None` when the selection is coherent (including when it has no off-chain
/// tool, or no provider that declares a seam).
pub fn check(assignments: &[RoleAssignment], registry: &Registry) -> Option<Incompatibility> {
    let off_chain = assignment_tool(assignments, Role::OffChain, registry)?;
    let devnet = assignment_tool(assignments, Role::Devnet, registry);

    // A self-hosting off-chain tool + a separate devnet: the devnet is redundant
    // (and can't be its provider). Reported against the devnet specifically.
    // (`devnet` is `Option<&ToolDef>`, which is `Copy`, so it stays usable below.)
    if let Some(d) = devnet.filter(|_| off_chain.compat.self_contained_devnet) {
        return Some(Incompatibility {
            off_chain_id: off_chain.id.clone(),
            off_chain_name: off_chain.name.clone(),
            reason: format!(
                "{} provides its own embedded devnet, so a separate devnet isn't used",
                off_chain.name
            ),
            self_hosted: true,
            providers: vec![provider_ref(d)],
            compatible_providers: compatible_providers_for(off_chain, registry),
            compatible_off_chain: off_chain_for(&[d], registry),
        });
    }

    // Providers = the devnet (if any) + every selected infra tool, keeping only
    // those that actually declare a served seam.
    let providers: Vec<&ToolDef> = devnet
        .into_iter()
        .chain(
            assignments
                .iter()
                .filter(|a| a.role == Role::Infrastructure)
                .filter_map(|a| registry.get(&a.tool_id)),
        )
        .filter(|p| !p.compat.serves.is_empty())
        .collect();

    // Permissive when the off-chain tool declares no seams, or nothing selected
    // declares a provider seam (supplementary infra / no devnet — the tool falls
    // back to a public provider per the interface contract).
    if off_chain.compat.consumes.is_empty() || providers.is_empty() {
        return None;
    }

    let served: HashSet<Seam> = providers
        .iter()
        .flat_map(|p| p.compat.serves.iter().copied())
        .collect();
    if off_chain.compat.consumes.iter().any(|s| served.contains(s)) {
        return None; // at least one selected provider can feed the off-chain tool
    }

    Some(Incompatibility {
        off_chain_id: off_chain.id.clone(),
        off_chain_name: off_chain.name.clone(),
        reason: format!(
            "{} speaks {}, which none of the selected providers serve",
            off_chain.name,
            seam_list(&off_chain.compat.consumes),
        ),
        self_hosted: off_chain.compat.self_contained_devnet,
        providers: providers.iter().map(|p| provider_ref(p)).collect(),
        compatible_providers: compatible_providers_for(off_chain, registry),
        compatible_off_chain: off_chain_for(&providers, registry),
    })
}

fn assignment_tool<'a>(
    assignments: &[RoleAssignment],
    role: Role,
    registry: &'a Registry,
) -> Option<&'a ToolDef> {
    assignments
        .iter()
        .find(|a| a.role == role)
        .and_then(|a| registry.get(&a.tool_id))
}

fn provider_ref(tool: &ToolDef) -> ProviderRef {
    ProviderRef {
        id: tool.id.clone(),
        name: tool.name.clone(),
    }
}

/// Devnet/infra tool ids in the registry whose served seams overlap the
/// off-chain tool's consumed seams.
fn compatible_providers_for(off_chain: &ToolDef, registry: &Registry) -> Vec<String> {
    let mut ids: Vec<String> = registry
        .all_tools()
        .iter()
        .filter(|t| {
            t.roles.contains_key(&Role::Devnet) || t.roles.contains_key(&Role::Infrastructure)
        })
        .filter(|t| {
            t.compat
                .serves
                .iter()
                .any(|s| off_chain.compat.consumes.contains(s))
        })
        .map(|t| t.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Off-chain tool ids whose consumed seams overlap the union of these providers'
/// served seams.
fn off_chain_for(providers: &[&ToolDef], registry: &Registry) -> Vec<String> {
    let served: HashSet<Seam> = providers
        .iter()
        .flat_map(|p| p.compat.serves.iter().copied())
        .collect();
    let mut ids: Vec<String> = registry
        .tools_for_role(Role::OffChain)
        .iter()
        .filter(|o| o.compat.consumes.iter().any(|s| served.contains(s)))
        .map(|o| o.id.clone())
        .collect();
    ids.sort();
    ids
}

fn seam_list(seams: &[Seam]) -> String {
    seams
        .iter()
        .map(|s| s.label())
        .collect::<Vec<_>>()
        .join(" or ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        Registry::load().expect("registry should load")
    }

    fn a(role: Role, tool: &str) -> RoleAssignment {
        RoleAssignment {
            role,
            tool_id: tool.to_string(),
        }
    }

    #[test]
    fn blockfrost_offchain_with_yaci_is_compatible() {
        let reg = registry();
        let sel = vec![a(Role::OffChain, "evolution"), a(Role::Devnet, "yaci")];
        assert!(check(&sel, &reg).is_none());
    }

    #[test]
    fn mesh_with_dolos_infra_is_compatible() {
        let reg = registry();
        // Mesh speaks UTxORPC; Dolos infra serves it.
        let sel = vec![
            a(Role::OffChain, "meshjs"),
            a(Role::Infrastructure, "dolos"),
        ];
        assert!(check(&sel, &reg).is_none());
    }

    #[test]
    fn evolution_with_kupmios_is_compatible() {
        let reg = registry();
        let sel = vec![
            a(Role::OffChain, "evolution"),
            a(Role::Infrastructure, "kupo"),
            a(Role::Infrastructure, "ogmios"),
        ];
        assert!(check(&sel, &reg).is_none());
    }

    #[test]
    fn evolution_with_dolos_infra_is_incompatible() {
        let reg = registry();
        // Evolution has no UTxORPC provider, so Dolos (u5c only) can't feed it.
        let sel = vec![
            a(Role::OffChain, "evolution"),
            a(Role::Infrastructure, "dolos"),
        ];
        let inc = check(&sel, &reg).expect("evolution + dolos infra should be incompatible");
        assert_eq!(inc.off_chain_id, "evolution");
        assert!(inc.providers.iter().any(|p| p.id == "dolos"));
    }

    #[test]
    fn compatible_devnet_saves_an_incompatible_infra() {
        let reg = registry();
        // Yaci feeds Evolution (Blockfrost); an extra Dolos infra doesn't break it.
        let sel = vec![
            a(Role::OffChain, "evolution"),
            a(Role::Devnet, "yaci"),
            a(Role::Infrastructure, "dolos"),
        ];
        assert!(check(&sel, &reg).is_none());
    }

    #[test]
    fn supplementary_infra_imposes_no_constraint() {
        let reg = registry();
        // cardano-node serves no off-chain seam, so it never conflicts.
        let sel = vec![
            a(Role::OffChain, "evolution"),
            a(Role::Infrastructure, "cardano-node"),
        ];
        assert!(check(&sel, &reg).is_none());
    }

    #[test]
    fn no_offchain_is_compatible() {
        let reg = registry();
        assert!(check(&[a(Role::Devnet, "yaci")], &reg).is_none());
    }
}
