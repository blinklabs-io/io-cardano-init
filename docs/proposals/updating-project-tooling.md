# Proposal: Updating a scaffolded project's tooling & roles

**Status:** Accepted · **Last updated:** 2026-08-15 · **Owner:** Robertino Martinez

> **Implemented.** `cardano-init add`/`remove` ship this design: reconstruction in `doctor::probe::reconstruct`, the pure change-set in `scaffold::update`, the writer in `scaffold::writer::apply_update`, and the CLI edge in `cli::update`. See PRD FR-25 and TECH_SPEC §2.1/§2.5. The interactive `edit` command in §9 was **dropped** as low-value — `add`/`remove` (with `--dry-run`) cover the need.

> Closes the exploration in [#26](https://github.com/input-output-hk/cardano-init/issues/26). This proposal owns the *design* for editing an already-generated project's role/tool set. It builds on the interface contract (ARCHITECTURE §4 / TECH_SPEC §7), the scaffolding pipeline (ARCHITECTURE §6), and the doctor project scan (TECH_SPEC §9.6). Where behavior is not yet in code it is **(planned)**.

---

## 1. Motivation & scope

Users want to change a project *after* the initial scaffold (#26):

- swap a tool in a role (Aiken → Pebble, MeshJS → Tx3);
- add a role (on-chain-only → on-chain + off-chain);
- add infrastructure (a new provider as requirements grow);
- drop a role (narrow the scope).

**In scope.** Editing the *set of roles and the tool filling each* by **adding, removing, and re-rendering component folders**, then re-wiring the shared top-level files. The change is expressed as a mutation of the project's `Selection`.

**Out of scope (unchanged non-goals).** This is **not** a version/package manager: it does not pin, upgrade, or migrate the *tooling itself* (issue #26: *"We still won't handle updating the tooling itself"*), and it never rewrites the user's code inside a component it is keeping. It only adds/removes/replaces whole components and regenerates the generated wiring.

**PRD reconciliation.** PRD §5.2 currently states *"There is no `cardano-init update` … generated once and owned by the user thereafter."* Adopting this proposal narrows that non-goal to its true intent — *not a version manager* — while allowing role/folder-level edits. PRD §5.2 and ROADMAP (Phase 2/3) must be updated if this lands.

---

## 2. Decision: detection, not a persisted manifest

An update needs the project's **current** `Selection` as its base state. Two ways to get it:

1. **Persist a manifest** at scaffold time (serialize `Selection` into e.g. `.cardano-init/manifest.toml`) and read it back.
2. **Reconstruct by detection** — scan the project tree and infer the selection, exactly as `doctor` already does (TECH_SPEC §9.6, `doctor::probe::scan_project`).

**We choose detection.** Rationale:

- **Consistency.** `doctor` deliberately carries *no* metadata file — *"the project's structure is the source of truth"* (TECH_SPEC §9.6). A manifest for `update` would contradict a principle the project already committed to, and would introduce a second source of truth that can silently drift from the tree (user hand-deletes a folder → manifest lies).
- **Completeness is achievable.** Every field of `Selection` is recoverable from the tree (§4). The one historically "lossy" field — the **infra provider set** — is in fact present verbatim in `infra/Justfile` as `cardano-up install <package>` lines (§4.2).

Detection's one real risk — reconstruction logic coupled to generated template text, which could silently recover the wrong set — is mitigated structurally by the **confirm step** (§5): the tool never *trusts* its reconstruction; it shows it and asks. Reconstruction is a proposal to the user, not an authority.

> If reconstruction proves fragile in practice, a manifest can be added later as an *optimization* (a trusted fast-path that still falls back to detection). This proposal does not require it.

---

## 3. The model: components vs the shared layer

Every generated project is exactly two things, and the update engine treats them differently:

**Component slots** — one directory per contract role, plus the fused `protocol/` slot (contract.rs `DIR_*`):

| Slot | Dir | Multiplicity | On change |
|------|-----|--------------|-----------|
| on-chain | `on-chain/` | one tool | replace dir |
| off-chain | `off-chain/` | one tool | replace dir |
| infrastructure | `infra/` | **many tools, one aggregated dir** | re-render in place |
| devnet | `devnet/` | one tool | replace dir |
| formal-methods | `formal-methods/` | one tool | replace dir |
| protocol (fused) | `protocol/` | one fullstack tool | replace dir |

A component is **self-contained** and **standalone** (contract): its rendered content does not depend on which sibling roles are present. This is the load-bearing guarantee — **a slot whose tool is unchanged is never touched** (§7).

**Shared layer** — top-level files derived from the *whole* selection, re-rendered on any change: `Justfile`, `.env`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `.gitignore`, and (under `--nix`) `flake.nix` / `.envrc`, plus the `blueprint/.gitkeep` marker.

The update = **reconstruct → confirm → mutate → validate → diff the slots → re-render the shared layer → write under a git safety net.**

---

## 4. Phase A — Reconstruct the current `Selection`

Extend the existing `scan_project` (which already yields `{ComponentKind → tool_id}` for all five role dirs + `protocol/`, and flags `UnrecognizedDir`) into a full `Selection` reconstructor. It is an **impure-edge** concern (reads the tree), living beside `doctor::probe`.

```
reconstruct(root, registry) -> Reconstructed {
    selection: Selection,          // best-effort
    unrecognized: Vec<UnrecognizedDir>,
    low_confidence: Vec<Field>,    // fields we had to guess
}
```

### 4.1 Roles & tools
Directly from `scan_project`. Each `DetectedComponent` becomes a `RoleAssignment` (or, for `ComponentKind::Protocol`, the two assignments `{on-chain→T, off-chain→T}` that the planner re-collapses into `protocol/`).

### 4.2 Infrastructure providers
`scan_project` reports infra as a single synthetic `INFRA_DRIVER_ID` ("cardano-up"). Recover the actual set by parsing `infra/Justfile` for `cardano-up install <package> --context …` lines and mapping each `<package>` back to a tool via `ToolDef.infra.cardano_up_package`. Unknown packages are surfaced (not silently dropped) for the user to confirm in §5.

### 4.3 Network / Nix / project name
- **network** — parse `CARDANO_NETWORK=` from `.env`.
- **nix** — `flake.nix` present (and/or `.envrc`).
- **project_name** — the root directory name. (Only ever used to *render* text; a wrong guess is caught in confirm and is git-recoverable regardless.)

Any field not cleanly recoverable is marked `low_confidence` and always shown for confirmation.

---

## 5. Phase B — Confirm (the trust boundary)

Reconstruction is **never trusted silently**. Before any mutation:

- **Interactive** — print the reconstructed selection and any `unrecognized`/`low_confidence` items, and require the user to confirm or correct it. This is the seam that neutralizes detection's coupling risk.
- **Non-interactive / `--format json`** — no prompts. The caller must either (a) accept the reconstruction as-is *and* it must be fully recognized, or (b) supply the current selection explicitly via flags. **Any `unrecognized` dir is a hard error** (`project_unrecognized`, exit 2) — the tool refuses to guess in automation.

---

## 6. Phase C — Apply the mutation & validate

The mutation (`add`/`remove`/`swap`, or interactive re-selection) produces `S_new`. `S_new` is then run through **exactly the same gates as `init`** — no new validation logic:

- **Role uniqueness** — one tool per non-infra role; infra repeatable and **deduped keep-first** (parity with TECH_SPEC §3.4, so "add an infra provider already present" is an idempotent no-op).
- **At least one role** — removing the last role is refused (`no_roles_selected`, exit 2); a project is never left empty.
- **Compatibility gate** — `registry::compat::check(S_new.assignments, registry)`. If the resulting off-chain ↔ provider pairing shares no seam, stop with `incompatible_tools` (exit 2) unless `--ignore-warning` (parity with TECH_SPEC §3.2.2). *This is why the gate runs on the result, not the delta:* adding Dolos to an Evolution project, or swapping MeshJS→Tx3 next to a Yaci devnet, must trip the same check a fresh scaffold would.
- **Experimental gate** — if `S_new` newly includes an experimental tool, require `--allow-experimental` / interactive confirm (`experimental_not_allowed`, exit 2; parity with §3.2.1).

Because `Selection`-validity is by construction (ARCHITECTURE §3.3), a validated `S_new` is a first-class selection indistinguishable from one `init` would have built.

---

## 7. Phase D — Compute the change set

Pure logic over `S_old`, `S_new`, and the registry (belongs in `scaffold/`, alongside the planner). For each **slot** compare the tool in `S_old` vs `S_new`:

| Transition | Action |
|-----------|--------|
| absent → present | **CREATE**: plan+render that component into its dir. Precondition: the dir must not already exist on disk; if it does (a foreign/unrecognized dir), abort (`slot_occupied`). |
| present → absent | **REMOVE**: `rm -rf <dir>` (removes user-added files in that dir too — intentional; git is the net). |
| present → present, tool changed | **REPLACE**: REMOVE then CREATE. |
| present → present, tool same (non-infra) | **KEEP** — never touched. |
| infra: provider set changed (dir stays) | **RE-RENDER IN PLACE**: overwrite `infra/`'s managed files (`Justfile`, `README.md`, `scripts/write-env.sh`) from the new provider set. |
| infra → empty | **REMOVE** `infra/`. |

**Fusion boundary** is just rows in this table — no special "migration":
- two dirs (`on-chain`+`off-chain`) → fullstack tool = REMOVE both + CREATE `protocol/` (a full replace; nothing is carried over — the code the user wrote in the two dirs is deleted, loudly, git-recoverable).
- `protocol/` → separate tools = REMOVE `protocol/` + CREATE the new dir(s). "Drop the off-chain half of a fused `protocol/`" is expressed as *replace `protocol/` with an on-chain tool* — the user must name that on-chain tool; there is no in-place split of one tool's fused codebase.

**Shared layer** is always recomputed from `S_new` and written **per-file with a content diff** — a file is only rewritten if its bytes change (so a slot swap that doesn't alter, say, `.gitignore` leaves it alone). `blueprint/.gitkeep` is added/removed to match the predicate `any(role != Infrastructure)` (TECH_SPEC §6.2); `flake.nix`/`.envrc` follow the (unchanged, here) nix flag.

**The Aiken-is-safe guarantee.** Removing off-chain from `{on-chain→aiken, off-chain→meshjs}`: on-chain is `KEEP` (same tool), so `on-chain/` is never in the write set; the standalone-component contract guarantees aiken's render doesn't depend on the removed sibling, so even the content-diff shows no change. Only `off-chain/` is removed and the shared layer re-wired.

---

## 8. Phase E — Safety, atomicity & dry-run

The current writer is a blind, non-atomic `fs::write` loop that assumes an empty dir (TECH_SPEC §6.4, "no `--force`, never overwrites"). Updating writes into a **populated, user-owned** tree, so the update path adds guards *around* the writer rather than changing `init`'s policy:

- **Git safety net (required).** The update refuses to run on a **dirty** working tree (uncommitted changes) so that every create/overwrite/delete is reviewable and revertible via `git diff` / `git restore`. Overridable with `--force` for users who accept the risk. A **non-git** project is treated like a dirty tree: refuse unless `--force`, and warn that there is no undo.
- **This replaces a merge engine.** Managed/shared files are overwritten outright; the user reconciles any hand-edits through git. We build no three-way merge and no `.new` shadow files.
- **Write ordering for crash-safety.** (1) create & overwrite all new/changed files; (2) `rm -rf` removed dirs; (3) nothing else to persist (no manifest). A crash mid-run can leave *extra* files but never loses a kept component; re-running is idempotent.
- **`--dry-run`.** Prints the reconstructed `S_old`, the resulting `S_new`, and the full change set (CREATE / REMOVE / REPLACE / RE-RENDER / shared-file overwrites) and writes nothing — the auditable plan for humans and agents. `--format json` emits the same as structured data.

---

## 9. CLI surface

```
cardano-init add    --off-chain tx3    # one-shot: add/replace a slot (same flag vocabulary as init)
cardano-init add    --infra ogmios     # repeatable; dedup keep-first
cardano-init remove --off-chain        # one-shot: drop a role (by role, not tool)
cardano-init remove --infra kupo       # drop one infra provider
```

- `add`/`remove` reuse `oneshot`'s per-role flag parsing and validation verbatim.
- Global flags honored: `--dry-run`, `--ignore-warning`, `--allow-experimental`, `--format`, plus new `--force`.
- Deliberately **not** verbs like `swap`: an `add --off-chain X` onto an occupied off-chain slot *is* the swap (REPLACE), reported as such in the diff so it is never silent.
- An interactive `edit` (re-open the selector seeded from the detected stack) was considered and **dropped** as low-value: `add`/`remove --dry-run` already cover editing the stack, and `edit` duplicated the whole interactive stack for little gain.

---

## 10. Error catalog & exit codes

Reuses existing codes where the gate is shared (`unknown_tool`, `tool_role_mismatch`, `fullstack_*`, `no_roles_selected`, `experimental_not_allowed`, `incompatible_tools`, `invalid_network`). New codes (TECH_SPEC §2.5 table):

| `code` | Exit | `context` fields |
|--------|------|------------------|
| `project_unrecognized` | 2 | `{ dirs: [..] }` — a component dir couldn't be identified; in `json`/non-interactive this is fatal |
| `slot_occupied` | 2 | `{ role, dir }` — CREATE target dir already exists and isn't the recognized current tool |
| `nothing_to_change` | 2 | `{ }` — the mutation is a no-op against the reconstructed selection |
| `worktree_dirty` | 1 | `{ path }` — uncommitted changes (or not a git repo) and no `--force` |

Interactive **abort** (declining the confirm/diff) exits `0`, never in `json` mode — parity with §2.3.

---

## 11. Exhaustive edge-case matrix

| # | Situation | Handling |
|---|-----------|----------|
| 1 | Add a role not present | CREATE new dir + re-wire shared layer. |
| 2 | Add a tool to an occupied non-infra slot | REPLACE (remove+create); shown as a swap in the diff; destructive → git-gated. |
| 3 | Add an infra provider already present | Dedup keep-first → `nothing_to_change` (idempotent). |
| 4 | Remove a role that isn't present | `nothing_to_change`. |
| 5 | Remove the **last** non-infra role | Allowed; `blueprint/.gitkeep` dropped (predicate flips). |
| 6 | Remove on-chain while consumers remain | Allowed; warn that `blueprint/plutus.json` won't be produced (consumers already degrade gracefully per contract). |
| 7 | Remove the last remaining role | Refused — `no_roles_selected`. |
| 8 | Swap that breaks off-chain↔provider compat | `incompatible_tools` unless `--ignore-warning` (gate on `S_new`). |
| 9 | Add/swap in an experimental tool | `experimental_not_allowed` unless `--allow-experimental`. |
| 10 | Two dirs → fullstack tool (fuse) | REMOVE `on-chain/`+`off-chain/`, CREATE `protocol/`. Full replace, git-recoverable; not a merge. |
| 11 | `protocol/` → drop off-chain half | REPLACE `protocol/` with a user-named on-chain tool; no in-place split. |
| 12 | Same tool on both roles, no `[fullstack]` | Two separate dirs (not fused); each removable independently. |
| 13 | Infra provider set changes | RE-RENDER `infra/` in place from the new set; other slots untouched. |
| 14 | Unchanged slot (e.g. Aiken while off-chain changes) | KEEP — provably untouched (§7). |
| 15 | Unrecognized/renamed/foreign component dir | Interactive: surface for correction. Non-interactive: `project_unrecognized` (fatal). |
| 16 | Ambiguous detection (2+ tools match one dir) | Treated as unrecognized (existing `scan_project` behavior). |
| 17 | `infra/Justfile` hand-edited so providers unparseable | Recovered set shown in confirm; user corrects; non-interactive → treat as unrecognized. |
| 18 | Unknown `cardano-up` package in `infra/Justfile` | Surfaced, not dropped; user confirms/removes. |
| 19 | `.env` missing/edited `CARDANO_NETWORK` | Field marked low-confidence; asked/defaulted in confirm. |
| 20 | Dirty git tree / not a git repo | `worktree_dirty` unless `--force`. |
| 21 | Crash mid-write | Creates-before-deletes ordering → no kept component lost; re-run is idempotent. |
| 22 | `--dry-run` | Prints `S_old`, `S_new`, change set; writes nothing. |
| 23 | Slot swap that leaves a shared file byte-identical | Content diff skips it — no spurious rewrite. |

---

## 12. Architecture fit & module placement

- **Reconstruction** (reads the tree) is impure-edge, extending `doctor::probe` — reuses `scan_project`, adds infra-provider parsing + `.env`/nix/name recovery. Keeps the `registry`/`scaffold`/`contract` purity invariant intact.
- **Change-set computation** is pure logic over `S_old`/`S_new`/registry, living in `scaffold/` next to the planner; it emits an `UpdatePlan { creates, replaces, removes, rerenders, shared_overwrites }`.
- **Gates** are the existing pure functions (`compat::check`, the experimental predicate) — called on `S_new`, unchanged.
- **Apply** is the only side-effecting step: the CLI edge wraps the git-clean check, the confirm, and the (extended) writer.
- **Determinism** (TECH_SPEC §11) is preserved: reconstruction + canonical planning are deterministic, so `dry-run` output and the applied change set are reproducible.

No new dependency on `cli/` from the core; the `Role` vocabulary and the contract are untouched.

---

## 13. Phasing

1. **Reconstruction + `--dry-run` diff** — `reconstruct()`, the change-set computer, and a read-only `add`/`remove --dry-run` that prints `S_old`/`S_new`/diff. No writes; independently useful and testable, and improves `doctor`'s infra reporting for free.
2. **`add` (non-destructive)** — CREATE + shared re-wire + infra RE-RENDER, git-gated. Covers "expand scope" and "add infra" with no deletions.
3. **`remove` / swap (destructive)** — REMOVE/REPLACE + fusion transitions, behind git-clean + confirm.

---

## 14. Testing strategy

- **Round-trip:** scaffold `S`, `reconstruct()` → assert it equals `S` for every tool/role/infra/network/nix permutation in the existing matrix.
- **Change-set unit tests:** for representative `(S_old, S_new)` pairs assert the exact `UpdatePlan` (esp. the KEEP guarantee: unchanged slots produce zero writes; fusion transitions produce the expected remove/create set).
- **Golden tree tests:** apply an update to a scaffolded temp dir and assert the resulting tree matches a from-scratch `init` of `S_new` for all *managed* files (user code in kept components excluded).
- **Gate parity:** the same `(compat, experimental)` fixtures used for `init` produce the same errors through the update path.
- **Safety:** dirty-tree refusal; crash-ordering (deletes-after-creates) leaves no lost kept component.
