# lockmerge

A git merge driver that merges lockfiles by what they mean, not by their lines.

> **Status: pre-alpha.** There is no `lockmerge` binary yet and nothing to
> install. The Cargo workspace has two library crates: `lockmerge-core`
> (the three-way engine, proofs and refusal reports) and `lockmerge-cargo`
> (the `Cargo.lock` parser, resolved model, base rules and canonical writer).
> The plan is in [docs/charter.md](docs/charter.md), the research is in
> [docs/](docs/), and later work and known risks are listed in
> [docs/04-backlog.md](docs/04-backlog.md).

## The problem

Two branches each add a dependency. Each side's lockfile change is
independent, but the new entries land next to each other, so git reports a
conflict in a generated file nobody is meant to edit by hand.

The quieter failure is worse: git merges the lines cleanly and leaves a
lockfile that is internally inconsistent, such as a dependency edge that
points at a package entry neither side kept
([rust-lang/cargo#2302](https://github.com/rust-lang/cargo/issues/2302)).
The build then breaks on a branch where nobody touched a dependency.

## What lockmerge does

lockmerge is a single static Rust binary that git calls as a
[merge driver](https://git-scm.com/docs/gitattributes#_defining_a_custom_merge_driver).
For each lockfile, it parses the base, ours and theirs versions into a
dependency model, merges the models three-way, checks that the result is
consistent, and writes it back in the package manager's exact format.

### The rule: never guess

lockmerge combines what is provably unambiguous and refuses everything else.
When it can't prove a merge is correct, it exits non-zero and git leaves its
usual conflict markers, so the outcome is never worse than today. A silently
wrong lockfile is the one outcome lockmerge must never produce.

## Planned formats

In the order they are planned:

| Format | Package manager |
|---|---|
| `Cargo.lock` | Cargo |
| `uv.lock` | uv |
| `pnpm-lock.yaml` | pnpm |
| `package-lock.json` | npm |
| `go.sum` | Go modules |
| `yarn.lock` | Yarn (classic and berry) |
| `poetry.lock` | Poetry |
| `Gemfile.lock` | Bundler |

## How it will plug in (planned)

The commands below are a sketch; the final interface comes out of the
research in [docs/charter.md](docs/charter.md).

```sh
lockmerge install          # register the driver in this repository
lockmerge install --global # or for every repository
```

which amounts to git configuration like:

```gitattributes
# .gitattributes
Cargo.lock      merge=lockmerge
pnpm-lock.yaml  merge=lockmerge
```

```sh
git config merge.lockmerge.driver "lockmerge merge %O %A %B %P"
```

## Not in scope for v1

- Resolving genuine version conflicts by re-running a package manager's
  resolver.
- Editing manifests (`Cargo.toml`, `package.json`, ...).
- Network access during a merge. lockmerge never calls a registry or a
  package manager while merging.

## Related work

lockmerge builds on others' work on this problem. A full comparison will be in
`docs/00-prior-art.md`.

- [derived-merge](https://github.com/ShawezA/derived-merge): a semantic
  merge driver for npm, yarn and deno lockfiles, distributed through npm.
- [mergiraf](https://codeberg.org/mergiraf/mergiraf): a general syntax-aware
  merge driver.
- [cargo-merge-driver](https://github.com/relrelb/cargo-merge-driver):
  resolves `Cargo.lock` by regenerating it with `cargo generate-lockfile`.
- [pnpm/merge-driver](https://github.com/pnpm/merge-driver) and
  [npm-merge-driver](https://github.com/npm/npm-merge-driver) (archived):
  drivers for a single package manager that re-run the package manager.
- [awesome-merge-drivers](https://github.com/jelmer/awesome-merge-drivers):
  a list of merge drivers for many file types.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: see
[SECURITY.md](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
