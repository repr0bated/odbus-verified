# Kiro Spec Workflow

This repository uses Kiro to generate implementation specs under `.kiro/specs/`.
Use Kiro when you want a concrete spec package before code changes.

## When to Use It

Use Kiro for work that benefits from a written contract first:

- new host services
- schema or transport changes
- plugin boundary changes
- migration plans
- task breakdowns for another agent to implement

## Recommended Flow

Generate the spec in four passes:

1. Requirements
2. Design
3. Technical spec
4. Tasks

Keep each pass narrow. Ask Kiro to write only the current document, then move
to the next one after the previous output is acceptable.

## Practical Setup

Before running Kiro:

- make sure `kiro-cli` is installed and updated
- make sure `kiro-cli-term` is available in the terminal session
- run the dotfiles integration if Kiro asks for it
- prefer a separate comparison folder when you want to compare Kiro output with
  a manual draft

Useful checks:

```bash
kiro-cli --version
kiro-cli doctor -a
kiro-cli integrations status dotfiles
```

If the terminal integration is missing, start the wrapper in the same shell:

```bash
kiro-cli-term -- kiro-cli doctor -a
```

## Prompt Pattern

Start with requirements, then continue in order:

```text
Create requirements for <feature>.
Create design for the same feature.
Create technical spec and task list for the same feature.
```

For comparison work, tell Kiro to write into a separate folder, for example:

```text
Create a new spec folder at .kiro/specs/<name>-kiro/ and write:
requirements.md, design.md, spec.md, tasks.md, and .config.kiro.
```

## Boundary Rules

When writing specs for this repository:

- plugin schema stays in the plugin
- projection layers may read plugin data, but do not redefine it
- zeroclaw must not duplicate schema already handled elsewhere
- btrfs subvolumes are for local install/cache/rollback state, not duplicate schema trees
- transport layers should be explicit about native socket vs HTTP/gRPC-Web paths

## Comparing Outputs

If you want to compare Kiro with a manual draft:

1. create the manual draft in one folder
2. generate the Kiro version in a second folder
3. compare requirements, design, spec, and tasks side by side
4. keep the version that is tighter and more implementation-ready

This repo’s preference is to let Kiro write the spec files and then use the
manual draft only as a check, not as a competing authority.

