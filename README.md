# opencode-model-picker

Alt+M fuzzy picker for [opencode](https://opencode.ai) models. Reads the TOML
snapshot written by
[opencode-go-limits](https://github.com/roquib/opencode-go-limits)
(`~/.config/usages_limit/models.toml`) and lets you pick a model with its per-5h
request budget, then runs `opencode --model <id>`.

![Alt+M picker showing top models with per-5h limits](assets/choose-your-top-models.png)

## Prerequisites

- `opencode` on `PATH`
- `fzf` for the interactive picker (falls back to a plain prompt without it)
- A populated `~/.config/usages_limit/models.toml` (run `opencode-go-limits --once`)
- zsh for the `Alt+M` binding (the binary also works standalone — see `omodel`)

## Install

```bash
./install.sh
# verbose output:
./install.sh --verbose
```

This builds the release binary, installs it to
`~/.local/bin/opencode-model-picker`, installs shell completions, copies the
zsh widget to `~/.local/share/opencode-model-picker/opencode-model.zsh`, and
adds a source line to your `.zshrc` (after oh-my-zsh/fzf, where ZLE exists).

> Requires `~/.local/bin` on your `PATH`. Check with
> `echo $PATH | grep -q "$HOME/.local/bin" && echo OK || echo MISSING`.
> If missing, add it once:
>
> ```bash
> echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc && source ~/.bashrc
> # zsh: echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
> ```

### Set up Alt+M (zsh)

`install.sh` appends this to your `.zshrc` (preferred location
`${ZDOTDIR:-$HOME}/.zshrc`):

```zsh
[[ -f "$HOME/.local/share/opencode-model-picker/opencode-model.zsh" ]] \
  && source "$HOME/.local/share/opencode-model-picker/opencode-model.zsh"
```

The widget registers itself as:

```zsh
zle -N _omodel_widget
bindkey '^[m' _omodel_widget
bindkey '\em' _omodel_widget
```

Then:

```bash
exec zsh
# Press Alt+M → fzf picker, Enter runs `opencode --model <picked>`
```

(`^[m` / `\em` is Alt+M. If it does nothing, your terminal may not send Meta —
check with `showkey -a` or remap the Meta/Alt key.)

Manual helper (works with or without the keybinding):

```bash
omodel          # pick; puts `opencode --model <id>` in history (Ctrl+P recall)
omodel --exec   # pick and immediately exec opencode
```

## Usage

```bash
opencode-model-picker            # pick, print `opencode-go/<slug>` id
opencode-model-picker --list     # list models with usage stats, no picker
opencode-model-picker --exec     # pick and exec `opencode --model <id>`
opencode-model-picker --no-validate   # skip `opencode models` validation
opencode-model-picker --backend fzf|dialoguer|auto
opencode-model-picker --completions zsh|bash|fish
```

Without validation (`--no-validate` or empty cache) only the primary
`opencode-go/<slug>` candidate is offered. With validation, known
`opencode/<slug>-free` and `opencode/<slug>` aliases are offered too, using a
6h cache at `~/.cache/opencode-model-picker/valid_models.txt`.

## How it works

1. Loads the latest date entry from `~/.config/usages_limit/models.toml`.
2. Sorts by 5h limit descending (`Unlimited` first).
3. Validates candidates against `opencode models` (cached, refreshed in background).
4. Presents them via `fzf` (`--ansi`, `Model ❯` prompt) or `dialoguer` fallback.
5. Prints the id (for the zsh widget) or execs `opencode --model <id> -- ...`.

## Development

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
cargo build --release
```

Widget source lives at [`widgets/opencode-model.zsh`](widgets/opencode-model.zsh);
`install.sh` copies it — edit the repo copy, not the installed one.

## License

MIT — see [LICENSE](LICENSE).
