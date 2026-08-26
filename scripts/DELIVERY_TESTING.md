# Universal delivery regression testing

FnScribe's production delivery code contains no application names, bundle IDs,
or terminal-specific branches. The opt-in `delivery-harness` Cargo feature uses
that exact production path to insert a unique marker into whichever editable
control is focused after a three-second delay.

Quit the regular FnScribe app, then run:

```sh
./scripts/test-delivery.sh "a native macOS text editor"
./scripts/test-delivery.sh --reuse "a terminal prompt"
./scripts/test-delivery.sh --reuse "a browser textarea"
./scripts/test-delivery.sh --reuse "an Electron editor"
```

The first command builds and signs the feature-gated harness. `--reuse` keeps
the remaining probes fast while still verifying that bundle's signature.

For every target, confirm that the printed marker appears exactly once, the
previous clipboard is restored, and the target remains focused. The harness
build lives under `target/delivery-harness`, is excluded from release builds,
records only the target PID, marker byte count, success state, and any delivery
error, and exits after one probe.

The four target classes intentionally exercise capabilities rather than named
applications. Use any representative app in each class; adding a new app never
requires changing FnScribe's runtime delivery logic.
