#!/usr/bin/env bash
# Build a release binary and install it to ~/.local/bin/opencode-model-picker.
# Also install zsh widget for Alt+M and shell completions.
set -euo pipefail

VERBOSE=false
for arg in "$@"; do
    case "$arg" in
        -v|--verbose) VERBOSE=true ;;
    esac
done

log() {
    if [[ "$VERBOSE" == true ]]; then
        echo "[verbose] $*"
    fi
}

BIN_NAME="opencode-model-picker"
INSTALL_BIN="$HOME/.local/bin/$BIN_NAME"
WIDGET_SRC="$(dirname "$0")/widgets/opencode-model.zsh"
WIDGET_FILE="$HOME/.local/share/opencode-model-picker/opencode-model.zsh"

cd "$(dirname "$0")"
log "Building release binary..."
cargo build --release
log "Installing binary to $INSTALL_BIN"
install -Dm755 "target/release/$BIN_NAME" "$INSTALL_BIN"
echo "installed: $INSTALL_BIN ($("target/release/$BIN_NAME" --version 2>/dev/null || echo "ok"))"

# Install shell completions
log "Installing shell completions..."

# Bash
bash_dir="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions"
log "Creating bash completion dir: $bash_dir"
mkdir -p "$bash_dir"
log "Generating bash completions to $bash_dir/$BIN_NAME.bash"
"target/release/$BIN_NAME" --completions bash >"$bash_dir/$BIN_NAME.bash" 2>/dev/null && echo "  bash completions: $bash_dir/$BIN_NAME.bash" || echo "  skip bash completions"

# Zsh
zsh_dir="${ZDOTDIR:-$HOME}/.zsh/completions"
log "Creating zsh completion dir: $zsh_dir"
mkdir -p "$zsh_dir"
log "Generating zsh completions to $zsh_dir/_$BIN_NAME"
"target/release/$BIN_NAME" --completions zsh >"$zsh_dir/_$BIN_NAME" 2>/dev/null && echo "  zsh completions: $zsh_dir/_$BIN_NAME" || echo "  skip zsh completions"

# Fish
fish_dir="$HOME/.config/fish/completions"
log "Creating fish completion dir: $fish_dir"
mkdir -p "$fish_dir"
log "Generating fish completions to $fish_dir/$BIN_NAME.fish"
"target/release/$BIN_NAME" --completions fish >"$fish_dir/$BIN_NAME.fish" 2>/dev/null && echo "  fish completions: $fish_dir/$BIN_NAME.fish" || echo "  skip fish completions"

# Install Zsh widget (Alt+M) from the repo copy
log "Installing zsh widget to $WIDGET_FILE"
mkdir -p "$(dirname "$WIDGET_FILE")"
install -Dm644 "$WIDGET_SRC" "$WIDGET_FILE"
echo "  zsh widget: $WIDGET_FILE (Alt+M)"

# Ensure .zshrc sources the widget after oh-my-zsh (ZLE available there).
# Prefer $ZDOTDIR/.zshrc, then ~/.zshrc. Legacy dotfiles layout
# (~/dots/zsh/.zshrc) is checked last so existing setups keep working.
ZSHRC=""
for candidate in "${ZDOTDIR:-$HOME}/.zshrc" "$HOME/.zshrc" "$HOME/dots/zsh/.zshrc"; do
    if [[ -f "$candidate" ]]; then
        ZSHRC="$candidate"
        break
    fi
done
if [[ -z "$ZSHRC" ]]; then
    ZSHRC="${ZDOTDIR:-$HOME}/.zshrc"
fi
SRC_LINE="[[ -f \"$WIDGET_FILE\" ]] && source \"$WIDGET_FILE\""
if ! grep -qF 'opencode-model.zsh' "$ZSHRC" 2>/dev/null; then
    echo "" >> "$ZSHRC"
    echo "# opencode-model-picker: Alt+M widget (requires ZLE; sourced here after oh-my-zsh/fzf)" >> "$ZSHRC"
    echo "$SRC_LINE" >> "$ZSHRC"
    echo "  zshrc: added source line for widget ($ZSHRC)"
else
    echo "  zshrc: source line already present ($ZSHRC)"
    echo "  note: if your source line points at an old path (e.g. ~/dots/...),"
    echo "  update it to: $SRC_LINE"
fi

echo ""
echo "Installation complete!"
echo "  Binary: $INSTALL_BIN"
echo "  Widget: $WIDGET_FILE  → Alt+M in any Zsh session"
echo "  Models: ~/.config/usages_limit/models.toml (populated by opencode-go-limits)"
echo ""
echo "Try it:"
echo "  1) Reload shell:  exec zsh  (or source $WIDGET_FILE)"
echo "  2) Press Alt+M    → fzf picker, Enter to run opencode --model <picked>"
echo "  3) Or run:        omodel --exec"
echo "     List only:     $BIN_NAME --list"
