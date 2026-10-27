---
name: 3tched
description: Explains the difference between 3tched (our own code, features, and architecture) and threetched (the unmodified zeroclaw release), and how 3tchedfs / threetchedfs mirror the blobs and replace RPC calls with disk I/O writes. Use this skill whenever the user mentions 3tched, threetched, zeroclaw, 3tchedfs, threetchedfs, the blob mirror, the removed bridge code, or asks you to add, change, audit, or replace code in 3tched. Also use it before writing ANY new logic in 3tched, because the central rule is that nothing is hand-rolled if threetched already offers it. Use it even if the user does not name the rules explicitly.
---

# 3tched vs threetched

## The two names

| Name | What it is | Who owns it |
|---|---|---|
| **3tched** | Our code, our features, our architecture | Us. We change it. |
| **threetched** | The zeroclaw release, derived directly from the zeroclaw binary | Upstream. Unmodified. We never change it. |

**zeroclaw = threetched.** zeroclaw is the original name of that release; threetched is what we call it. They are one codebase.

Any leftover "zeroclaw" label in 3tched (identifier, comment, doc string, log text, feature flag) is a label-only rename to threetched, with no behavior change.

## Core rule

**3tched contains no hand-rolled code for anything threetched offers.** If threetched provides a capability, 3tched uses threetched's implementation as is. 3tched is only what threetched does not already cover: our features and our architecture.

## threetched: unmodified zeroclaw release

- Its code is unmodified code from the zeroclaw binary/release.
- Never edit it, patch it, wrap it to change behavior, or "improve" it.
- It is the source of truth. If 3tched and threetched disagree in a category threetched covers, 3tched is wrong.

## 3tched: our code

3tched is where our own features and architecture live. It should be thin on anything generic. Before writing logic in 3tched:

1. **Check threetched first.** Read the threetched source for the same job. Do not decide from names or memory.
2. **If threetched has it, use it unchanged.** Call it, do not reimplement it.
3. **If it is only a label,** rename to the threetched term. No behavior change.
4. **If threetched has no equivalent,** it is legitimately ours to write. Confirm by reading threetched, then write it as a 3tched feature.
5. **If existing 3tched code is hand-rolled** in a category threetched covers, it is a defect. Remove it, switch call sites to threetched, and fix imports and `Cargo.toml`.
6. **If behavior differs** between the 3tched code and the threetched equivalent, change nothing. Report the differences and ask.

### What counts as hand-rolled

Custom logic (parsing, protocol handling, state, error handling, retries, serialization, plumbing) in 3tched that duplicates something threetched provides. Thin glue that only calls threetched is fine.

## 3tchedfs / threetchedfs: the blob mirror

Both filesystem layers share the same design:

- **A mirror of the blobs.** The on-disk layout reflects the blob store faithfully. It is not a new data model. No extra structure, indexes, or transformations the blobs do not have.
- **Replaces RPC calls with disk I/O writes.** Where the old design sent an RPC (the removed bridge between the binary and 3tched), state now goes through writes to disk.

Naming (confirm with the user if unsure): **3tchedfs** is ours, in 3tched. **threetchedfs** is the corresponding layer in the threetched release and follows the same unmodified rule as the rest of threetched.

### Consequences

- **No RPC** in 3tched or either filesystem layer. A call that looks like bridge leftovers is a candidate for removal or for replacement with a disk write.
- **Disk is the interface.** Communication and persistence go through the mirrored files, not a side channel.
- **Blobs are the source of truth.** If a blob changes shape, the mirror follows.
- **Writes use threetched.** If threetched provides file and write handling (atomic writes, locking, flushing), use it. Do not hand-roll it.

## Decision checklist

1. RPC or RPC-shaped code? Replace with a disk write through 3tchedfs, or remove if it was bridge-only.
2. Logic in a category threetched covers? Use threetched unmodified.
3. Only a name or label, including the word zeroclaw? Rename to threetched.
4. threetched lacks an equivalent? It is a 3tched feature. Write it, keep it minimal.
5. Unsure? Make no change and report what you found and the question you could not resolve.

## Never

- Never modify threetched.
- Never hand-roll code in 3tched for something threetched offers.
- Never add RPC.
- Never make the filesystem layers more than a mirror of the blobs.
- Never delete 3tched code without confirming the threetched equivalent exists and behaves the same.
