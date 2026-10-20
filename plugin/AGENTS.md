# Lyrics translation plugin: install, update, and repair playbook

This file is for coding models and maintainers who keep this fork working
across upstream releases. Read it before installing, updating, or changing
the plugin.

## What the plugin is

Spotifast has no runtime plugin API, so the official binary cannot load code
from outside. Upstream declined translated lyrics (crmne/spotifast#930 and
#534); the feature therefore lives here as a small commit series on top of an
upstream release:

- branch `lyrics-translation`, based on a release tag such as `v0.13.3`;
- `src/lyrics_translation.rs` plus the wiring listed at the end of this file;
- `plugin/install.sh`, the one-command installer and updater;
- the rebuilt binary at `<repo>/target/release/spotifast`, which is the path
  the desktop launcher uses.

`plugin/install.sh` keeps the previous build as
`<repo>/target/release/spotifast.previous` for rollback. It writes install
metadata to `~/.local/state/spotifast/plugin-install` (or under
`$XDG_STATE_HOME`).

A true runtime plugin would need an API added upstream. Do not present this
as one; describe it as a patched build that installs and updates with one
command.

## Ground rules

- Keep the branch linear. Rebase onto release tags; never create merge
  commits.
- Never log, print, or commit the translation API key. It lives only in the
  app's state directory, read inside the network task.
- Never delete or weaken the plugin's tests to make an update pass.
- Never push at all unless the user asks. When they do, push only to the
  fork remote (`fork` in the maintainer checkout, `origin` in a plain clone
  of the fork), and rewrite the branch only with
  `git push --force-with-lease <remote> lyrics-translation`.
- Keep a working binary and a backup branch before risky work. If a step is
  uncertain, stop, leave the running build alone, and report.
- Two bidi tests, `bidi::tests::lines_are_drawn_in_bidi_order` and
  `bidi::tests::wrapped_and_cut_rows_are_drawn_in_bidi_order`, fail in some
  environments because of font shaping. They fail the same way on unmodified
  upstream in this environment. Treat only those two as known; every other
  failure blocks an update.

## Quick paths

```sh
./plugin/install.sh                 # build and install the current branch
./plugin/install.sh --update        # rebase on the newest release, check,
                                    # build, and install
./plugin/install.sh --status        # versions, checkout, launcher, backup
./plugin/install.sh --repair        # abort a stopped rebase, reinstall if needed
./plugin/install.sh --rollback      # restore the previous binary
./plugin/install.sh --export-patches DIR
```

`--update --tag vX.Y.Z` updates to a specific release. `--no-fetch` uses the
tags already in the clone. `--skip-checks` is for emergencies only; say so
when you use it.

### What `--update` does

1. Requires a clean checkout and switches to `lyrics-translation`.
2. Fetches tags from a remote that points at `crmne/spotifast`, or from its
   URL directly when the clone has no such remote.
3. Records a `plugin-backup-<date>` branch.
4. Rebases `--onto <newest tag> <base tag>`.
5. Refreshes `assets/i18n` and commits the result if it changed.
6. Runs `cargo fmt --all --check`, clippy with `-D warnings`, the full test
   suite (tolerating only the two known bidi failures), and the catalog check.
7. Backs up the installed binary, runs `cargo build --release --locked`,
   verifies that the new binary contains `Clear translations`, and writes the
   install state.

The running app keeps its old copy until the user restarts it. Say so.

If the rebase conflicts, the script aborts it, restores the branch,
installs nothing, and prints the manual steps. Continue below.

## Updating to a new release

Start with the automatic path:

```sh
./plugin/install.sh --status
./plugin/install.sh --update
```

If it succeeds, go to "Verification". If it stops on a rebase conflict, the
branch is back where it was. Rebase by hand:

```sh
git switch lyrics-translation
git fetch origin --tags
git rebase --onto vX.Y.Z vOLD
```

`vOLD` is the `Base tag` from `--status`. Resolve one conflict at a time with
the map below, then:

```sh
cargo fmt --all
git add <resolved files>
git rebase --continue
```

When the rebase finishes, run the full path again. Because the base is now
the new tag, it skips the rebase and does the catalog refresh, checks, build,
and install:

```sh
./plugin/install.sh --update
```

If a check fails, fix the cause, commit nothing extra unless it is a real
change, and rerun. If the failure looks wrong or the conflict touches more
than a few lines, stop: keep the working binary, report what you found, and
ask the user before forcing a design choice.

## Conflict map

Keep upstream's surrounding code and re-insert the plugin's pieces. The
feature was written to stay in small, separable blocks.

| File | What to preserve |
| --- | --- |
| `src/lyrics_translation.rs` | New, self-contained module. It should apply cleanly; if upstream added a file with the same name, stop and ask. Keep providers, free-model filters, prompts, retries, cache prefix, and tests intact. |
| `src/settings.rs` | `TranslateProvider`, `lyrics_translate`, `lyrics_translate_provider`, `lyrics_translate_model`, `lyrics_translate_language`, their defaults, and serde attributes that keep old `settings.json` files readable. |
| `src/paths.rs` | `translation_key_file()` under the app's state dir. |
| `src/model.rs` | `TranslationRequest` (`uri`, `lines`, `language`, `provider`, `model`, `prefetch`) and `Event::LyricsTranslation`. |
| `src/backend.rs` | `Command::TranslationModels`, `Command::TranslateLyrics`, `Command::SetTranslationKey`; their events; `Worker::translate_lyrics`, model listing, key storage; the prefetch retry and its `log::warn!`. The key is read from disk inside the task, never sent in a command. |
| `src/app.rs` | Translation and prefetch state fields and their initialisers; lyrics event routing; `request_lyrics`, `request_translation`, `prefetch_next_lyrics`, `keep_prefetched_lyrics`, `take_prefetched_lyrics`; the track-change prefetch call; queue refresh while lyrics are open; key/model actions and toasts; the related tests. |
| `src/ui/lyrics.rs` | Globe button in the panel header and the fullscreen header; translation drawn under each original line; loading, failed, and retry states; never replace upstream's layout or scrolling. |
| `src/ui/settings.rs` | The **Lyrics translator** section (provider, key, model, language) and the Storage row **Lyric translations** with **Clear translations**. Keep upstream's ordering and styling conventions. |
| `src/demo.rs` | Translation demo surfaces and the headless regression tests. Update selectors only if upstream renames its demo plumbing. |
| `assets/i18n/` | Regenerate, do not hand-merge. The feature's msgids are `Clear translations`, `Lyrics translator`, `Translate lyrics`, and the settings labels and descriptions. |

Upstream also owns `README.md` and `AGENTS.md`; this fork's own notes live in
`FORK-NOTES.md`, `plugin/README.md`, and this file. Keep fork-only prose out
of upstream files except the single fork section in `README.md`.

## Translation catalogs

After a rebase, line references and upstream strings change. Run:

```sh
bash .github/scripts/update-translations.sh
bash .github/scripts/update-translations.sh --check
```

The script requires GNU gettext tools. If they are missing, install them
(for example `gettext` on Arch and Debian, `gettext` with Homebrew on macOS).
Never hand-edit `.po` files when the generator can do it. Never drop the
plugin's msgids.

## Verification

1. `./plugin/install.sh --status` shows the new base tag, a clean checkout,
   the installed version, and `Plugin: present in the installed binary`.
2. Ask the user to close and reopen Spotifast (the old process keeps the old
   binary until then), then check:
   - the globe appears in the lyrics panel and fullscreen view and toggles;
   - translations sit under the original lines, smaller and dimmer;
   - a song already in the target language shows no second line;
   - with translations on, the next song's translation is already there when
     it starts;
   - **Settings → Lyrics translator** keeps the provider, key presence, free
     model, and language;
   - **Settings → Storage → Lyric translations** can clear the cache.
3. If the app misbehaves, read the log:
   `~/.local/state/spotifast/spotifast.log`. Look for
   `could not prepare the translation`, an API-key rejection, or a rate
   limit. Never paste the key or the lyrics into an issue.

## Repair matrix

| Symptom | Cause and fix |
| --- | --- |
| `--update` stops with "rebase stopped on a conflict" | Expected when upstream moved the code. The branch was restored and nothing was installed. Follow "Updating to a new release" and the conflict map. |
| The checkout has uncommitted changes | The update refuses to touch it. Commit or stash, then rerun. Never `git checkout -- .` without reading the diff. |
| `cargo` is missing | Install Rust and the system libraries from upstream's Build from source guide, then rerun `--status`. |
| mise refuses to run `cargo` in a fresh checkout | Run `mise trust` once in that checkout, then rerun the script. |
| Fetch fails | Run `--update --no-fetch` only if the needed tag is already local; otherwise fix the network or remote and retry. |
| Build fails on a system library | Install the dependency upstream lists for this platform; do not patch the plugin to work around it. |
| Clippy or fmt fails | Run `cargo fmt --all` and fix the lint. Rerun the update; it skips the rebase because the base is current. |
| Tests fail beyond the two bidi tests | Read `target/plugin-tests.log`. Fix the plugin or adapt the test only when upstream intentionally changed the asserted behaviour. Never delete the test. |
| Catalog check fails | Run `bash .github/scripts/update-translations.sh`, check the diff, commit it, and rerun. Missing gettext is a common cause. |
| The app starts but has no globe | The installed binary is not the plugin build. Run `./plugin/install.sh`, then restart the app. `--status` should say `Plugin: present`. |
| The globe works but nothing translates | Check the key in Settings, the provider, and the model; check the log for 401/429; clear stored translations if a stale cache is suspected. |
| The launcher opens an old build | `--status` prints the binary the launcher points at. Make its `Exec` line match `<repo>/target/release/spotifast`, or start that path directly. |
| A stopped rebase is left over | `./plugin/install.sh --repair` aborts it and restores the branch. It then reinstalls the binary if it is missing or not the plugin build. |
| The update made things worse | `./plugin/install.sh --rollback` restores the previous binary, then restart the app. The branch and source are untouched by the rollback. |

## Done means

- the branch is linear and based on the chosen release tag;
- catalogs are regenerated, committed, and the catalog check passes;
- fmt, clippy, and the tests pass (except the two known bidi failures);
- the new binary is installed and verified, and the user has restarted the
  app and confirmed the feature;
- if the user asked to publish it, the branch is pushed to the fork remote
  with `--force-with-lease`, and the backup branch is left in place.
