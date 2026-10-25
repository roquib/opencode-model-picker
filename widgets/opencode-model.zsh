# opencode-model-picker — Alt+M widget
# Reads ~/.config/usages_limit/models.toml (populated by opencode-go-limits:
# https://github.com/roquib/opencode-go-limits)
# and lets you fuzzy-pick a model, then runs `opencode --model <id>`.

_omodel_bin="${HOME}/.local/bin/opencode-model-picker"

# Fallback helper for non-interactive / manual use:
#   omodel        → pick and put `opencode --model <id>` into history (Ctrl+P recall)
#   omodel --exec → pick and immediately exec opencode
function omodel() {
  local sel
  if [[ ! -x "$_omodel_bin" ]]; then
    echo "opencode-model-picker not found at $_omodel_bin" >&2
    echo "Run install.sh from the opencode-model-picker repo, then reload your shell." >&2
    return 1
  fi
  # Force tty for fzf when called outside zle
  sel="$("$_omodel_bin" </dev/tty)"
  local ret=$?
  if [[ $ret -ne 0 || -z "$sel" ]]; then
    return $ret
  fi
  if [[ "$1" == "--exec" ]]; then
    exec opencode --model "$sel" "${@:2}"
  else
    # Put command into Zsh history buffer without executing immediately
    # so user can edit before enter, but also print it
    if [[ -n "${ZSH_VERSION:-}" ]]; then
      print -z "opencode --model $sel"
    else
      echo "opencode --model $sel"
    fi
  fi
}

# ZLE widget for Alt+M — runs opencode directly with selected model
function _omodel_widget() {
  if [[ ! -x "$_omodel_bin" ]]; then
    zle -M "opencode-model-picker not found: $_omodel_bin — run install.sh"
    return 1
  fi
  local sel
  # `</dev/tty` is critical: when invoked as a ZLE widget stdin is not a tty,
  # but fzf needs a tty for interactive UI. We still feed candidates via pipe inside the binary.
  sel="$("$_omodel_bin" </dev/tty)"
  local ret=$?
  if [[ $ret -ne 0 || -z "$sel" ]]; then
    zle redisplay
    return 0
  fi
  # If command line is empty, replace it and execute immediately.
  # If user already typed something, insert model id at cursor and show hint.
  if [[ -z "$BUFFER" ]]; then
    BUFFER="opencode --model $sel"
    zle accept-line
  else
    # Insert model id at cursor position
    LBUFFER+="$sel"
    zle -M "inserted: $sel  (Alt+M)"
  fi
  zle redisplay
}
# Only register ZLE widget/binding when ZLE is available (interactive shell).
# .zshenv sources this file for all shells (including non-interactive AI agents)
# where zle is unavailable — guard avoids errors there. Real interactive binding
# is also ensured from .zshrc after oh-my-zsh loads.
if [[ -o interactive ]] && (( ${+functions[zle]} )) 2>/dev/null; then
  zle -N _omodel_widget 2>/dev/null
  bindkey '^[m' _omodel_widget 2>/dev/null
  bindkey '\em' _omodel_widget 2>/dev/null || true
elif [[ $- == *i* ]]; then
  if command -v zle >/dev/null 2>&1; then
    zle -N _omodel_widget 2>/dev/null
    bindkey '^[m' _omodel_widget 2>/dev/null
    bindkey '\em' _omodel_widget 2>/dev/null || true
  fi
fi
