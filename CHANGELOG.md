# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [v0.2.1] - 2026-08-20

### Documentation

- Keep v-prefix in changelog headings and document PR-based release flow (#73)
- Refresh README with a stronger hook and demo GIF (#76)

### Features

- Add --table for a compact tools-by-role matrix (#77)

### Fixes

- Detect tx3 despite @meshsdk dep, and silence git note (#74)

## [v0.2.0] - 2026-08-18

### Documentation

- Add installer-recipes CI badge to README
- Document the Gift Card reference example
- Correct inaccuracies vs implementation

### Features

- Add `fullstack` option that combines `on-chain` and `off-chain` roles in a single folder
- Implement Scalus on-chain, off-chain, and fullstack templates
- Add scalus to smoke test CI
- Add CI to verify dependency installers
- Clean pychache and allow running CI on selected PRs
- Add Plinth + restructure --nix generation to have composable flakes
- Make direnv allow non-interactive with a warning for non-trusted substituters
- Change validator to match default "Gift Card" example
- Gate experimental tools behind explicit opt-in (#11)
- Scalus templates use the Gift Card example, blueprint-driven off-chain
- Add Evolution SDK
- Data-driven off-chain ↔ provider compatibility
- Add Tx3 off-chain template (Gift Card)
- Remove unnecessary comments
- Generate AGENTS.md tailored to the selected stack
- Redesign CLI output and errors
- Add devnet smoke badge to README
- First draft on update/remove tooling
- Add git initialization, remove `cardano-init edit`, and improve help message
- `cardano-init add` replaces without prompt if git tree is clean
- Fold updating-project-ooling to main docs
- Fold updating-project-ooling to main docs
- Add more tests about updating tools functionality
- Fix network to preview, switch via CARDANO_NETWORK in .env
- Drop the web builder surface

### Fixes

- Don't crash installer verify on unstattable bin dir
- Correct broken doctor recipes and harden installer-recipes gate
- Detect tx3 off-chain (#58)
- Add FromStr impl for Network to fix clippy build
- Canonicalize nix_packages order (#65)

### Refactor

- Void datum + off-chain seed, dropping the redundant params

## [v0.1.0] - 2026-07-10

### Features

- Add `cardano-init doctor`
- Add flake.nix
- Add `cardano-init list` command
- Update README
- Add nix installer (closes #2)
- Improve install/usage instructions
- Update tests and docs
- Add cargo dist to hanble the releases
- Ignore .claude
- Added npm to publish platforms

### Fixes

- Remove redundant borrow in println (clippy 1.97)

[v0.2.1]: https://github.com/input-output-hk/cardano-init/compare/v0.2.0..v0.2.1
[v0.2.0]: https://github.com/input-output-hk/cardano-init/compare/v0.1.0..v0.2.0
[v0.1.0]: https://github.com/input-output-hk/cardano-init/releases/tag/v0.1.0

