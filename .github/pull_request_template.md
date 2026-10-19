<!--
Thanks for contributing to ScribeCraft. Base this on `main`, the only integration branch.
Delete any section below that does not apply.
-->

## What this changes

<!-- What behaviour is different afterwards, and why. `Fixes #123` if it fixes an issue. -->

## How you verified it

- [ ] `cargo fmt --check`
- [ ] clippy with `-D warnings` on the crates I changed
- [ ] tests pass on the crates I changed
- [ ] `cargo xtask layers`, `cargo xtask assets`, `cargo xtask parity`
- [ ] for audio changes: rendered with `transport_render` / `scribecraft-cli render` and listened

**Tests.** New behaviour needs a test at the right scale. For a bug fix, **check that the
test fails before your fix** by reverting the fix and watching it go red.

## Never-crash and real-time safety

Every recording, script, settings file, pedal report and keystroke is untrusted (`AGENTS.md` §4).

- [ ] No new `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!` outside tests unless provably infallible (with a comment)
- [ ] No indexing with input-derived positions; checked arithmetic; capped allocations; NaN speeds handled
- [ ] No `unsafe`
- [ ] Nothing new in the audio callback allocates, blocks or can panic

## Provenance

- [ ] Nothing taken from Express Scribe or any NCH product (code, help text, icons, sounds), and nothing read from its installation beyond file names
- [ ] No real recordings of people; test media is synthetic (`AGENTS.md` §1.2)
- [ ] No font files committed (they live in storytold/craft-fonts)
- [ ] No GPL/AGPL code copied in; behaviour from public docs, specs or black-box observation
- [ ] Every new asset has an `ATTRIBUTION.toml` entry and `cargo xtask assets` passes

## Security and privacy

- [ ] `--root` confinement still holds on every path, including nested script steps
- [ ] MCP, the control channel and any network intake stay opt-in
- [ ] No new outbound network access (that decision is the owner's)
- [ ] No dictation names, paths or content logged at info level or above

## Parity and reach

- [ ] `parity/express-scribe-features.toml` updated, with `notes` saying what is *still* missing
- [ ] Reachable headlessly through an `automation` tool where that applies
- [ ] Docs updated if a documented behaviour changed

## Anything a reviewer should look at first
