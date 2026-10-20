# Lyrics translation plugin

This fork adds translated lyrics to Spotifast: a globe in the lyrics header
shows a translation under each original line, in the side panel and the
fullscreen view. It works with OpenRouter or Google AI Studio and your own
free API key.

Spotifast has **no runtime plugin API**. The official binary cannot load code
from outside, so this "plugin" is a small commit series on top of an upstream
release, built into a binary by the script here. Installation and updates are
still one command.

The feature itself is described in [../FORK-NOTES.md](../FORK-NOTES.md).
Coding models and maintainers get the full update and repair playbook in
[AGENTS.md](AGENTS.md).

## Requirements

- Linux or macOS, and the build dependencies from Spotifast's
  [Build from source](https://spotifast.rocks/getting-started/#build-from-source)
  guide (Rust, the system audio libraries).
- GNU gettext tools, needed to check and refresh the translation catalogs
  during updates.
- This checkout of the fork, on the `lyrics-translation` branch.
- Network access for the first build and for updates.

## Install

```sh
git clone --branch lyrics-translation https://github.com/SwoverZ/spotifast.git ~/spotifast-fork
cd ~/spotifast-fork
./plugin/install.sh
```

The script builds the release binary, replaces it on disk while the running
app keeps the old copy, keeps the previous build as
`target/release/spotifast.previous`, checks that the translation feature is
present, and prints the path. Close Spotifast and open it again to run the
new build.

If an existing checkout already has this directory, just run
`./plugin/install.sh` inside it.

In a fresh checkout, mise may refuse to run `cargo` until its config is
trusted. Run `mise trust` once inside the checkout and rerun the script. This
only affects mise users; plain Rust installations skip it.

## Update

```sh
./plugin/install.sh --update
```

One command does the whole job:

1. fetch the newest upstream release tag;
2. rebase the plugin branch onto it;
3. refresh the translation catalogs;
4. run formatting, clippy, the test suite, and the catalog check;
5. back up the current binary, build, verify, and install.

`--update --tag v0.14.0` chooses a specific release. If the rebase hits a
conflict, nothing is installed and the branch is restored; the script prints
the manual steps and points to [AGENTS.md](AGENTS.md), which has a conflict
map and a repair matrix.

Expect updates to need attention when upstream moves the files this feature
touches. The playbook is written so a coding model can do it end to end.

## Status, repair, rollback

```sh
./plugin/install.sh --status
./plugin/install.sh --repair
./plugin/install.sh --rollback
```

- **status** shows the branch's base release, the newest release, the
  checkout state, the installed binary and its version, whether the plugin is
  present, and the launcher path.
- **repair** aborts a stopped rebase, restores a clean branch, and reinstalls
  the binary if it is missing or is not the plugin build.
- **rollback** swaps the previous binary back in; the build you came from
  becomes the backup, so you can roll forward again.

## What the script changes on disk

| Path | What it is |
| --- | --- |
| `<repo>/target/release/spotifast` | The installed binary the desktop launcher uses. |
| `<repo>/target/release/spotifast.previous` | The previous binary, used by `--rollback`. |
| `<repo>/target/plugin-tests.log` | The last full test log (only after an update). |
| `~/.local/state/spotifast/plugin-install` | Installed tag, commit, time, and binary path. |
| `~/.local/state/spotifast/translation_key` | Your API key, owner-only mode. Never touches the repository. |

The script does not change the desktop file. It warns if no `.desktop` entry
launches the binary.

## Export the plugin as patches

To apply the feature to a clean upstream checkout elsewhere:

```sh
./plugin/install.sh --export-patches /path/to/patches
```

This writes the commit series against the current base tag. Apply it with
`git am` in a checkout of that tag.

## Troubleshooting

| Problem | What to do |
| --- | --- |
| Update stopped on a conflict | Nothing was installed. Follow [AGENTS.md](AGENTS.md), section "Updating to a new release". |
| The app has no globe | Install again with `./plugin/install.sh` and restart the app. `--status` must say `Plugin: present`. |
| The globe works but there is no translation | Check the provider, API key, model, and language in **Settings → Lyrics translator**; check the log below; clear translations from Storage if the cache is stale. |
| The app opens an old build | The `.desktop` file launches a different path. `--status` prints the binary it uses, and `--repair` reports a mismatch. |
| A rebase was interrupted | `./plugin/install.sh --repair` aborts it and restores the branch. |
| The update broke playback or the interface | `./plugin/install.sh --rollback`, then restart the app. |
| Tests fail | Read `target/plugin-tests.log`. Only the two known bidi font-shaping failures are tolerated. |

Logs live at `~/.local/state/spotifast/spotifast.log`. The log is truncated
on each start, so reproduce the problem before reading it. Never paste the
API key or lyric text into a public issue.
