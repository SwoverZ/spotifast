# Spotifast fork: translated lyrics

This is a personal fork of [crmne/spotifast](https://github.com/crmne/spotifast)
based on v0.13.3. It adds a lyrics translator: under each original lyric
line, the side panel and the full-screen view show the translation in
smaller, dimmer text.

Upstream declined translated lyrics (crmne/spotifast#930 and #534), so the
feature lives here. Everything else tracks upstream; the branch is
`lyrics-translation`.

## Installing and updating

Spotifast has no runtime plugin API, so the feature is a small commit series
on top of an upstream release, built into a binary by one script. It installs
where the desktop launcher expects it and keeps the previous build for
rollback.

```sh
./plugin/install.sh            # build and install the current branch
./plugin/install.sh --update   # rebase on the newest release, check, install
./plugin/install.sh --status   # installed and available versions
./plugin/install.sh --repair   # recover from a failed install or update
./plugin/install.sh --rollback # restore the previous binary
```

[`plugin/README.md`](plugin/README.md) covers requirements, paths, and
troubleshooting. [`plugin/AGENTS.md`](plugin/AGENTS.md) is the full playbook
for coding models and maintainers: conflict map, verification, and the repair
matrix for a release update.

## What it does

- A globe button in the lyrics header, in the side panel and in the full
  screen view, turns the translator on and off. The button is accent
  coloured while it is on.
- Settings gains a **Lyrics translator** section:
  - **Provider**: OpenRouter (default) or Google AI Studio.
  - **API key**: typed in Settings and saved to a file only you can read
    (owner-only mode 0600 on Unix). It never goes into `settings.json`, the
    logs, the `Debug` output, or the settings UI, which only shows whether a
    key is saved.
  - **Model**: chosen from the free models the provider lists for your key.
    With no model chosen, the first free model is used and remembered.
  - **Language**: the language to translate into. **System language**
    follows the computer's language; otherwise pick one of the ISO 639-1
    languages, listed by their own name.
- Translations are cached per track, language, provider, and model in the
  lyrics cache directory as `translation-<sha1>.json`. They are kept between
  sessions, so a song is only sent to the provider once, until you clear them
  from **Settings → Storage → Lyric translations → Clear translations**.
- A song already in the target language is left alone: before translating,
  the model is asked whether the first lines are already in that language,
  and a yes shows no second line. The answer is cached with the translation,
  and a failed check translates as usual rather than guessing.
- The words of the next song in the queue are fetched, and translated when
  the translator is on, while the current song plays, so a new song shows
  them at once. This happens whether or not a lyrics view is open.

## Privacy

- Only the lyric lines and the target language travel to the provider you
  choose, in batches of at most 60 lines (at most 16 for the language check).
  No track metadata, no account information, nothing else.
- The API key is stored on this computer only:
  - Linux: `~/.local/state/spotifast/translation_key`
  - macOS and Windows: `translation_key` in the app's state directory.
- The key is read from that file inside the network task. It is never put in
  a URL (it travels in the `Authorization` header for OpenRouter and the
  `x-goog-api-key` header for Google, never as a query parameter), never
  logged, and never serialised into `settings.json`.
- Removing the key from Settings deletes the file and drops the in-memory
  translation state.
- The cache holds the lyric lines and their translations in the clear, like
  the lyrics cache upstream already does.
- The next song's lines are sent to the provider ahead of time while the
  translator is on, so its translation is ready when it starts. They are
  cached like any other translation, and the prefetch is skipped when the
  translator is off or offline. A failed prefetch is retried twice in the
  background; it never blocks the song that follows.

## Files this fork adds or changes

- `src/lyrics_translation.rs` (new, self-contained): providers, free-model
  lists, the prompt and its parsing, one retry on 429, 5xx, and answers that
  arrive cut short (a free provider can drop a generation halfway), and the
  on-disk cache, with unit tests against a local fake server. No new crates.
- Additions to existing files:
  - `src/settings.rs`: `lyrics_translate`, `lyrics_translate_provider`,
    `lyrics_translate_model`, `lyrics_translate_language`, all with defaults
    so old `settings.json` files keep working.
  - `src/backend.rs`: `TranslationModels`, `TranslateLyrics`, and
    `SetTranslationKey` commands with their events and tasks.
  - `src/app.rs`, `src/model.rs`: state, events, and actions, including
    `ClearTranslations` for the Storage section.
  - `src/ui/settings.rs`: the settings section and the Storage row with its
    **Clear translations** button.
  - `src/ui/lyrics.rs`: the globe buttons and the translated line rendering.
  - `src/paths.rs`: `translation_key_file()`.
  - `src/demo.rs`: `lyrics-translation`, `lyrics-translation-fullscreen`,
    and `lyrics-fullscreen-translation-view` demo surfaces, plus a headless
    regression test.
  - `assets/i18n/`: the new strings in all 15 catalogs.
  - `README.md`: a pointer to this file.
  - `plugin/`: the installer and updater, the human guide, and the model
    playbook for updates and repairs.

## Testing

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo run --features demo -- --demo --demo-show lyrics-translation
```

The provider code is covered against a local fake server: the key never
appears in a URL, only the lines are sent, and parsing accepts plain JSON,
code fences, and Google's `responseMimeType` shape.

Two upstream tests, `bidi::tests::lines_are_drawn_in_bidi_order` and
`bidi::tests::wrapped_and_cut_rows_are_drawn_in_bidi_order`, fail in some
environments because of font shaping differences; they fail the same way on
unmodified v0.13.3.

## Keeping it fresh with upstream

The one-command path is `./plugin/install.sh --update`, described in
[`plugin/README.md`](plugin/README.md). When a rebase conflicts, the command
installs nothing and restores the branch; follow
[`plugin/AGENTS.md`](plugin/AGENTS.md) for the conflict map and the manual
steps.

The raw command, for reference:

```sh
git fetch upstream --tags
git rebase --onto v0.14.0 v0.13.3 lyrics-translation
```

Resolve conflicts by keeping upstream's code around the translator
additions. The new settings fields use `#[serde(default)]` and
`skip_serializing_if` so both directions of `settings.json` keep working.
