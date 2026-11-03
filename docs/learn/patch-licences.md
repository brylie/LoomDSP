# Licences for shared patches

What a licence on a patch says, how a short SPDX name can stand for it, and why the patch browser's licence filters show fewer patches rather than more. For players who share patches, and developers who touch patch metadata.

- **Kind:** explainer.
- **Used by:** [patch metadata specification](../design/patch-metadata.md) (licence chooser, rules 6 and 9).
- **Decision behind it:** [ADR-0023](../adr/0023-patch-licences-spdx-and-creative-commons.md).
- **Code:** `crates/loom-patch/src/metadata.rs` (`Licence`, `LicenceFilter`, `Choice`); the chooser in `crates/loom-app/src/licence_chooser.rs`.

## Why a patch needs a licence at all

A patch is creative work. Without a licence, the law in most countries says that only its authors may copy or change it. Someone who downloads your drone patch and plays it in a set, or changes a knob and shares the result, is on uncertain ground, even if you would be happy with that.

A licence is the author's written answer to "what may others do with this?". LoomDSP never picks one for you (rule 7), because that answer is yours to give.

## Two questions are enough for most people

Creative Commons licences are built from a few building blocks:

| Block | Short name | Means |
|---|---|---|
| Attribution | BY | Credit the authors. Every licence below has it, except CC0. |
| Share-alike | SA | Share changed versions under the same licence. |
| No derivatives | ND | Share only unchanged copies. |
| Non-commercial | NC | No use for money. |

So two questions pick one of six licences: "May others change it?" (yes, yes with share-alike, or no) and "May they use it for money?" (yes or no). A seventh answer, CC0, gives up all rights. That is why the chooser asks two questions and not twenty.

For example, "yes, if they share it the same way" and "no money" give `CC-BY-NC-SA-4.0`: credit the authors, no commercial use, share changes under the same terms.

## SPDX: one short, exact name

[SPDX](https://spdx.org/licenses/) is a standard list of licence names, such as `MIT`, `Apache-2.0`, and `CC-BY-4.0`. Tools read these names without guessing. Writing `license: "CC-BY-4.0"` in a patch file means the same thing to LoomDSP, a code host, and a sharing site.

SPDX also has **expressions** that combine names:

| Expression | Means |
|---|---|
| `MIT OR CC-BY-4.0` | You may pick either licence. |
| `MIT AND CC-BY-NC-4.0` | Both apply at once. |
| `Apache-2.0 WITH LLVM-exception` | One licence with an extra permission added. |

## A wrong turn: guessing what a licence allows

The patch browser can show only patches that let you change them, or use them commercially. The obvious way is to look for words in the licence: no `NC` means commercial use is fine. That fails:

- `MIT AND CC-BY-NC-4.0` has `MIT` in it, but the `NC` part still applies. `AND` needs both sides to allow it.
- `my own terms` has no `NC`, but nobody knows what it allows.

So the filters read the expression as SPDX defines it. `A OR B` allows something when either side does, because you may pick the side you like. `A AND B` allows it only when both do. `WITH` adds a permission and takes none away, so it counts as its licence. A name the app does not know counts as "not allowed".

| Licence | Allows commercial use? | Why |
|---|---|---|
| `MIT OR CC-BY-NC-4.0` | Yes | You may pick MIT. |
| `MIT AND CC-BY-NC-4.0` | No | The NC part applies too. |
| `my own terms` | No | Unknown terms are treated as no. |
| none | No | No licence means no permission. |

*Why "no" for unknown:* a filter that promises reuse must never show a patch that forbids it. Missing a patch you could have used is a small loss. Using one you were not allowed to is a real problem for a performer.

## Open source licences

Licences such as `MIT`, `Apache-2.0`, and `GPL-3.0-or-later` are not Creative Commons, but by their own definitions they allow changes and commercial use. The SPDX list marks them as OSI-approved or FSF free, so the filters count them as allowing both. Conditions such as credit or share-alike still apply.

## How other systems do it

Patch-sharing sites often offer a Creative Commons choice when you upload. Package managers for code, such as Cargo, store an SPDX expression in each package. LoomDSP does both: Creative Commons questions for musicians, and an SPDX expression in the file for tools.

## Not legal advice

This page and the app explain licences in plain words. They do not give legal advice. Read the licence's own page before you rely on it.
