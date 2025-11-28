NOTE: This is early-stage software

# cargo-hawk

TUI that listens for file events and runs several programs in sequence (typically different cargo invocations).

## Features

* Auto-advance to the next program in sequence
* Built-in parsing for cargo output
* Color highlighting
* Automatic result ordering, puts failures before warnings
* Collapse output to get an overview, or show all the details
* Drop into $EDITOR to adjust your plan

## Todo

- [ ] Parse test outputs
- [ ] Add examples with warnings, errors, different plans
- [ ] Add cli args to supply custom plans

## Prior Art

* cargo-watch
* bacon

## Note on AI use

This software has been initially prototyped using Claude, but it has been largely refactored by hand since then.
