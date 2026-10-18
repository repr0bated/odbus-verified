# Cargo routing

JEV owns model selection and worker routing. Cargo has no separate model
allocation, caller-family mapping or LLM shell dispatcher.

Use the ordinary Cargo executable from the repository root. An assigned
execution worker runs its command without recursively delegating it. Keep one
Cargo process at a time and use the native `x86_64-unknown-linux-gnu` target.
Release builds retain `CXXFLAGS="-include cstdint"`. Workspace-wide commands
require an explicit workspace request; a Cargo run does not install binaries
or restart services.

The former repository dispatcher, shell alias, PATH shim and routing tests
have been removed. Existing shells that still inherited the old Cargo function
can clear it with `unset -f cargo; hash -r`; new shells no longer load it.

Implement, build, deploy. Clippy, formatting checks and the test suite remain
deferred until after deployment.

