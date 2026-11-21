# Cargo registry index entries and the local index cache

This document pins what the native-links proof (R15, V6 of
[cargo-lock.md](cargo-lock.md), [ADR 0008](../adr/0008-safe-merge-and-refusal-rules-for-cargo-lock.md)
decision 15) reads: the entry of one crate version in Cargo's local registry
index cache. Anything not pinned here is refused, never guessed.

Sources, checked on 2026-11-20:

- The registry index format: the Cargo Book, [Registry index](https://doc.rust-lang.org/cargo/reference/registry-index.html)
  (entry fields, file layout, sparse protocol). Cited below as "the Book".
- The cache file layout: not documented by the Book. It is an internal format of
  Cargo (`src/cargo/sources/registry/index/cache.rs` in rust-lang/cargo; the
  source was not re-read for this document). **The layout in section 3 was
  pinned by inspecting real files**, not from documentation: all 1,180 files in
  one `~/.cargo/registry/index/index.crates.io-*/.cache/` written by Cargo
  1.96.0 (2026-05-25), read by hand with a hex dump and a small script. Every
  file had the header and record structure of section 3.
- Statements marked **unverified** come from memory of Cargo's behavior or
  documentation and were not checked against a file or the source.

## 1. An index entry (the Book)

An entry is one JSON object per published version. The fields that matter:

| Field | Meaning | Used by lockmerge |
| --- | --- | --- |
| `name` | The crate name, in its original case | Must equal the lockfile name exactly |
| `vers` | The version, SemVer | Must equal the lockfile version string exactly |
| `cksum` | SHA-256 of the `.crate` file, lowercase hex | Must equal the lockfile `checksum` exactly |
| `links` | The `links` key of the version's `Cargo.toml`; absent or `null` if none | The value the proof reads |
| `yanked` | Whether the version is yanked | Ignored. It is the only field the Book says changes after publishing |
| `v` | The index schema version; absent means 1 | A value other than 1 or 2 is refused (the cache header of section 3 says 2 is the highest this layout covers) |
| `deps`, `features`, `features2`, `rust_version` | Dependencies and features | Not read |

A published version's `links` cannot change: the entry of a version is
immutable apart from `yanked` (the Book; this is also what ADR 0011 and the
architecture document rely on). The `cksum` of the entry is the SHA-256 of the
crate file the lockfile's `checksum` names, so an entry whose name, version and
`cksum` equal the lockfile's is the entry of that exact package.

## 2. Where the entry file is (the Book)

Index files are named by the crate name and grouped by its length:

| Name length | Path of the entry file |
| --- | --- |
| 1 | `1/<name>` |
| 2 | `2/<name>` |
| 3 | `3/<first character>/<name>` |
| 4 or more | `<first two characters>/<next two characters>/<name>` |

The Book says path components are lowercase. lockmerge reads only names made of
ASCII lowercase letters, digits, `-` and `_`. A name with an uppercase letter or
any other character is refused, because how the cache names such a file was not
verified (none of the 1,180 inspected files had an uppercase name).

Where Cargo keeps the registry index locally (the Cargo Book, "Cargo home"):
`$CARGO_HOME/registry/index/<registry>/`, where `CARGO_HOME` defaults to
`~/.cargo`. `<registry>` is a host name or URL with a hash, for example
`index.crates.io-1949cf8c6b5b557f`. The hash depends on the Cargo version, so
several directories for one registry can exist (section 5). lockmerge never
computes the name; it lists `registry/index/`.

## 3. The cache file (pinned by inspection)

For a sparse registry (`sparse+https://index.crates.io/`, the default since
Cargo 1.70), the entry file of a crate is cached at
`<index directory>/.cache/<path of section 2>`. The directory also holds
`config.json`. Verified on Cargo 1.96.0: `.cache/li/bz/libz-sys`,
`.cache/2/az`, `.cache/3/a/aes`.

The file is binary:

| Bytes | Content |
| --- | --- |
| 1 | The cache version: `0x03` |
| 4 | The highest index schema version the writer read, little-endian: `2` (`02 00 00 00`) |
| then | The index version string (for the sparse index an `etag: ...` or `last-modified: ...` HTTP header value), ended by one NUL byte |
| then, repeated | The version (`vers`) as text, a NUL byte, the JSON object of that version (one line, no newline), a NUL byte |

Seen in all 1,180 files: the first five bytes were `03 02 00 00 00`, the text
after the NUL of the index version string was a sequence of such pairs, and the
file ended with the NUL of the last JSON object (so the file split at NUL gives
an index version string, pairs, and one empty last piece). In the files read,
the version text in front of each JSON object was its `vers` (all 40,171
records), the file name was the lowercase `name`, `v` was absent or 2, and
`links` was absent or a string (never `null`, never empty).

lockmerge reads a cache file only if all of this holds, and otherwise treats the
file as unreadable:

- the first byte is 3 and the next four bytes are `02 00 00 00`;
- the index version string, then each pair, ends with a NUL, and the file ends
  right after a NUL ending a JSON object;
- the version text and the JSON are valid UTF-8, and the JSON object is parsed
  only for a record whose version text equals the version looked up;
- the parsed object has string `name`, `vers` and `cksum`, `vers` equals the
  version text in front of it, `links` is absent, `null` or a non-empty string,
  and `v`, if present, is 1 or 2.

Other cache versions (older Cargo wrote other header bytes; **unverified**
which), other highest-schema numbers, extra bytes, a truncated record and
anything else are refused. The cache format is internal to Cargo and may
change: a new Cargo that writes another header makes the proof refuse until this
document and the reader are updated. That is the safe direction.

**Git registries (unverified).** For a git index, such as the old crates.io
index `registry+https://github.com/rust-lang/crates.io-index`, Cargo keeps a
bare git repository in `registry/index/<name>-<hash>/`, whose JSON files are git
objects. They are not files lockmerge can read without running git, and it does
not. Cargo is believed to keep the same `.cache/` files, with the git commit id
as the index version string, in that directory. No git index directory was
available to check this. lockmerge uses the same reader for every directory and
refuses a file whose header it cannot read exactly, so an unverified layout
refuses instead of being guessed.

## 4. What lockmerge asks of one file

For a registry package (name, version, checksum from the lockfile): the lookup
passes if exactly one record has that version text, and its parsed `name`,
`vers` and `cksum` equal the three values. Two records with the same version, a
lockfile package with no checksum, or a record that does not parse, give no
match. The result is the record's `links`: a value, or none.

## 5. Several index directories

Every directory of `registry/index/` is searched, for the file of section 3
under it. A directory without the file, or whose file is unreadable by section
3, contributes nothing. The package is proven if at least one directory has a
matching record. If two directories each have a matching record and their
`links` differ, nothing is proven (an index entry of a published version cannot
change, so this means something is wrong). A registry that is not crates.io
leaves its own directory there; because a match needs name, version and the
hash of the crate file, a record from another registry matches only the same
package.

## 6. Not verified

- The Cargo source of the cache reader and writer was not read. The layout is
  from inspection of 1,180 files of one Cargo version on one machine.
- Git index directories (section 3), and caches written by Cargo versions other
  than 1.96.0, were not seen.
- Names with uppercase letters (section 2).
- `links = ""` or a `links` that is not a string (refused here, not seen).
- The Book's statement that the entry of a published version never changes
  besides `yanked` is relied on, not tested.
