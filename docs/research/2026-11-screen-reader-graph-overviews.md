# Overviews of Node-Link Diagrams for Screen Reader Users

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-09.
- **Question:** how can a screen reader user get an overview of a patch and trace a signal path quickly, without visiting every control? What do published research and standards say, and which accessibility relations reach a screen reader through AccessKit today?
- **Used by:** [canvas accessibility specification](../design/canvas-accessibility.md), [Reading a patch by ear](../learn/reading-a-patch-by-ear.md), UC11, NFR-UX-07.
- **Related:** [the accessibility spike](2026-11-a11y-beside-iced-spike.md), [the accessibility UX review](2026-11-accessibility-ux-review.md), [the large patch review](2026-11-large-patch-ux-review.md).

It is a dated snapshot. Do not update it to follow later releases; write a new one instead.

**Method and limits.** Sources are published papers, W3C documents, and the documentation pages of research projects. Clean room (ADR-0018): no source code of audio apps or UI frameworks was read, and no other audio product's manual or accessibility documentation. AccessKit is an accepted dependency, so its adapters' source was read (section 4). Facts marked † were seen only in a search summary or a secondary source, not on the page itself; check them before relying on them. No screen reader user was asked, and nothing here was tried with a screen reader.

## 1. Overview first

- **Shneiderman (1996)** gave the rule "overview first, zoom and filter, then details on demand" for visual information displays, and lists networks among the data types and *relate* among the tasks. Shneiderman, B. "The Eyes Have It: A Task by Data Type Taxonomy for Information Visualizations." Proc. IEEE Symposium on Visual Languages, 1996, pp. 336–343, DOI 10.1109/VL.1996.545307 †. The rule is cited, and applied to screen readers, by Zong et al. (below), which was read.
- **Zong et al. (2022)** studied screen reader access to charts and name three design dimensions: *structure* (list, table, or tree, and what each node stands for), *navigation* (structural moves up and down the tree, lateral moves between neighbouring branches, spatial moves, and targeted jumps from a menu), and *description* (content, order, and how much is said). Their participants wanted an overall picture first, and then to compare single items. Too much speech left users choosing between listening to all of it or stopping it; they recommend letting users control how much is said, and saying a level's name only when the level changes. Zong, J., Lee, C., Lundgard, A., Jang, J., Hajas, D., Satyanarayan, A. "Rich Screen Reader Experiences for Accessible Data Visualization." Computer Graphics Forum 41(3), EuroVis 2022. <https://vis.csail.mit.edu/pubs/rich-screen-reader-vis-experiences/>
- **Olli** turns a chart into a tree of text descriptions, presented as an ARIA tree view: coarse descriptions at the top, finer detail lower down †. Blanco, M., Zong, J., Satyanarayan, A. "Olli: An Extensible Visualization Library for Screen Reader Accessibility." IEEE VIS Posters, 2022. <http://vis.mit.edu/pubs/olli/>
- **Lundgard and Satyanarayan (2021)** sort what a description can say into four levels: how the chart is built; statistics and relations; perceived trends and patterns; and domain context. Blind and sighted readers differed significantly in which level they found most useful. Lundgard, A., Satyanarayan, A. "Accessible Visualization via Natural Language Descriptions: A Four-Level Model of Semantic Content." IEEE TVCG (Proc. VIS 2021), 2022, DOI 10.1109/TVCG.2021.3114770. <https://arxiv.org/abs/2110.04406>

**For a patch:** a summary at the top (counts, loops, what is not heard), then the modules, then their connections, with controls only on request. The summary is mostly level 2 content (counts and relations). A level 3 sentence ("an FM voice with a feedback loop") would need the patch's own description, which the patch info can hold; it is not proposed.

## 2. Navigating graphs, not only trees

- **Data Navigator** models any structure, including graphs and flows, as nodes and edges, and maps each kind of edge to a pair of navigation commands, one for each direction (for example "drill in" and "drill out", or "left" and "right"). Keyboard, screen reader, speech, and other input all become those commands. Elavsky, F., Nadolskis, L., Moritz, D. "Data Navigator: An Accessibility-Centered Data Navigation Toolkit." IEEE VIS 2023 / IEEE TVCG, DOI 10.1109/TVCG.2023.3327393. <https://arxiv.org/abs/2308.08475>, <https://dig.cmu.edu/data-navigator/api/>
- **TADA** makes node-link diagrams usable on a touch screen with tones and speech: one gesture gives an overview, another lists a node's links, and another follows one link to its neighbour, with a sound on arrival. Zhao, Y., Nacenta, M. A., Sukhai, M. A., Somanath, S. "TADA: Making Node-link Diagrams Accessible to Blind and Low-Vision People." CHI 2024, DOI 10.1145/3613904.3642222. <https://arxiv.org/html/2311.04502v4>. Its related-work section is the source for the next three entries' context.
- **Brown, Stevens, and Pettifer (2004)** discuss how to present graph-based diagrams without sight, and their molecule browser combined browsing by hierarchy with browsing along connections †. Brown, A., Stevens, R., Pettifer, S. "Issues in the non-visual presentation of graph based diagrams." Proc. IV 2004, pp. 671–676; and "Evaluation of a non-visual molecule browser." ASSETS 2004, pp. 40–47 †.
- **TeDUB** presented technical diagrams, including circuits, as a hierarchy with a spatial view of connected nodes; earlier studies found users wanted a meaningful entry point and a clear order †. Horstmann, M., et al. "Automated interpretation and accessible presentation of technical diagrams for blind people." New Review of Hypermedia and Multimedia, 2004 †.
- **Metatla, Bryan-Kinns, Stockman, and Martin** built a collaborative diagram editor whose audio view is a tree of at most four levels, with each node's edges as its children †. (CCmI project, DIS 2012 and BCS-HCI 2012 papers †.)
- **Blenkhorn and Evans (1998)** gave blind users access to data flow diagrams with a tactile overlay (cited in TADA; not read).
- **Schanzer, Bahram, and Krishnamurthi (2019)** read a program's structure aloud and move through its syntax tree by keys †, which supports moving through a patch by its structure rather than by screen position. "Accessible AST-Based Programming for Visually-Impaired Programmers." SIGCSE 2019 †.

**For a patch:** a cable is an edge with two directions, upstream and downstream, each with its own command. A jump lands with the target's name said at once, as TADA's arrival cue does. When a module has several cables on one side, the user picks; the commands never guess. Zong et al.'s targeted jump ("go to any module by name") is the find-a-module feature of #527; the outline's type-ahead covers part of it.

## 3. Standards

- **WAI-ARIA Authoring Practices, Tree View.** Down and Up move between visible nodes; Right opens a closed node or moves to the first child of an open one; Left closes an open node or moves to the parent; Home and End go to the ends; type-ahead is recommended, especially with more than seven root nodes; `*` may open all siblings. Level, position, and set size need setting only when not every node is present. <https://www.w3.org/WAI/ARIA/apg/patterns/treeview/>
- **Treegrid** adds cells to tree rows; Right opens a row and then moves into its cells. <https://www.w3.org/WAI/ARIA/apg/patterns/treegrid/>. Not proposed: a patch's rows have no common columns.
- **WAI-ARIA 1.2 relations.** `aria-flowto` names the next element or elements in an alternate reading order, and with several targets assistive technology should offer them as choices of path: the closest standard idea to following a cable. `aria-controls` names elements whose content or presence this one controls. `aria-details` points to an extended description. <https://www.w3.org/TR/wai-aria-1.2/>
- **Support for those relations on the web is poor.** `aria-flowto` was followed by only one screen reader and browser pair in older tests, and current support tables rate it partial †. `aria-controls` was announced only by JAWS, as a key command to jump to the controlled element (Pickering, H. "Aria-Controls is Poop", 2016, <https://www.heydonworks.com/article/aria-controls-is-poop>). In 2023 the ARIA working group noted it had no test cases for `aria-details` yet (<https://lists.w3.org/Archives/Public/public-aria/2023Oct/0014.html>). These are web results; a native app reaches the platform API directly, so section 4 matters more for LoomDSP.
- **WCAG 2.2** (<https://www.w3.org/TR/WCAG22/>): 1.1.1 asks for a text alternative for non-text content, and the WAI images tutorial says a complex image needs a long description of its relationships (<https://www.w3.org/WAI/tutorials/images/complex/>); 1.3.1 asks that relationships shown visually, such as cables, be available to assistive technology or as text; 2.1.1 asks that every function work from the keyboard; 2.4.3 asks for a focus order that keeps meaning; 2.4.5 asks for more than one way to find content, which by analogy supports an outline, a search, and following cables.

## 4. What reaches the screen reader through AccessKit

Read from source: `accesskit` 0.25.2, `accesskit_consumer` 0.39.2, and `accesskit_macos` 0.27.2 from the local Cargo registry. `accesskit_windows` 0.35.2 (which depends on `accesskit` 0.25.2) and `accesskit_atspi_common` 0.21.1 (used by the Linux adapter) were read on docs.rs as source pages, through a summarising tool, so their rows are marked †.

| AccessKit | macOS (`accesskit_macos` 0.27.2) | Windows (`accesskit_windows` 0.35.2) † | Linux (`accesskit_atspi_common` 0.21.1) † |
|---|---|---|---|
| `Tree`, `TreeItem` roles | Outline and outline row (`node.rs`, role table) | Tree and tree item | Tree and tree item |
| Rows of a tree | Only a tree's direct item children are its rows (`accessibilityRows` uses `items`, which does not descend into an item) | Yes | Yes |
| `level` | Not mapped: no disclosure level method | Level | Not found |
| `position_in_set`, `size_of_set` | Not mapped | Position in set, size of set | `posinset` and `setsize` attributes |
| Expanded state, Expand and Collapse actions | Not mapped | Expand and collapse pattern | Not found |
| `controls` | Linked UI elements (`node.rs:1161`), which VoiceOver can jump along | Controller for | Controller for relation |
| `flow_to` | Not mapped | Not mapped | Not found |
| `described_by`, `details` | Not mapped | Not mapped | Not found |
| Description | Accessibility help (`node.rs:543`) | Full description | Description |

"Not found" means the summarised source page showed no mapping; a later reading of the full file may find one.

**Findings:**

1. **`controls` is the only relation mapped on all three platforms.** It is a reasonable meaning for an output port's cable to an input (the output does control what the input receives), not for the reverse.
2. **A nested tree does not work as an outline on macOS today.** Rows nested inside rows are not listed as rows, and there is no expand, collapse, or level. An outline whose app handles its own keys, and whose structure is also said in words where it matters, works on every platform.
3. **The native `controls` support differs from the web results above.** VoiceOver's linked-element jump is a native feature, so the poor web support for `aria-controls` does not settle whether it works here. It needs a listening test.

## 5. Not proposed here

- **Sonification of the structure,** such as a tone per module or per signal type, as TADA uses on touch screens. Promising, but it needs its own study and spec.
- **Touch gestures.** LoomDSP is a desktop app first.
- **Generated level 3 descriptions** ("an FM voice"). They would need a model of what patches do, and could say things that are not true.

## Summary

The research supports four choices: an overview row first; a tree of modules and their connections with the standard tree keys; a command for each direction of a cable, with a choice when there are several; and short names with details after. The platform check adds a fifth: the app handles the outline's keys itself, and uses `controls` from output ports only as an extra path, because nothing else is mapped everywhere.
