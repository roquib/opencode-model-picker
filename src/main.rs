use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::{generate, Shell};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ModelUsage {
    name: String,
    #[serde(default)]
    slug: String,
    /// `None` means Unlimited for that window. Accepts a TOML integer,
    /// a numeric string, or "Unlimited" (any case) — mirrors opencode-go-limits.
    #[serde(
        default,
        deserialize_with = "de_opt_limit",
        serialize_with = "ser_opt_limit"
    )]
    requests_per_5h: Option<u64>,
    #[serde(
        default,
        deserialize_with = "de_opt_limit",
        serialize_with = "ser_opt_limit"
    )]
    requests_per_week: Option<u64>,
    #[serde(
        default,
        deserialize_with = "de_opt_limit",
        serialize_with = "ser_opt_limit"
    )]
    requests_per_month: Option<u64>,
}

fn ser_opt_limit<S: serde::Serializer>(
    v: &Option<u64>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    match v {
        Some(n) => s.serialize_u64(*n),
        None => s.serialize_str("Unlimited"),
    }
}

fn de_opt_limit<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<u64>, D::Error> {
    use serde::de::{self, Visitor};

    struct OptLimitVisitor;

    impl Visitor<'_> for OptLimitVisitor {
        type Value = Option<u64>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an integer, a numeric string, or \"Unlimited\"")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
            if v < 0 {
                Err(E::custom(format!("negative limit is not valid: {v}")))
            } else {
                Ok(Some(v as u64))
            }
        }

        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            if v.trim().to_lowercase().contains("unlimit") {
                return Ok(None);
            }
            let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return Err(E::custom(format!("No number found in: {v}")));
            }
            digits
                .parse::<u64>()
                .map(Some)
                .map_err(|_| E::custom(format!("Failed to parse number: {v}")))
        }

        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
            self.visit_str(&v)
        }
    }

    d.deserialize_any(OptLimitVisitor)
}

#[derive(Debug, Serialize, Deserialize)]
struct UsageData {
    models: BTreeMap<String, Vec<ModelUsage>>,
}

#[derive(Parser, Debug)]
#[command(
    name = "opencode-model-picker",
    version,
    about = "Pick opencode model (Alt+M) from ~/.config/usages_limit/models.toml"
)]
struct Cli {
    /// Print selected model id only (default)
    #[arg(long, default_value_t = false)]
    print: bool,

    /// After picking, directly exec `opencode --model <id>` (replaces picker process)
    #[arg(long, default_value_t = false)]
    exec: bool,

    /// List all available models (no picker) with usage stats
    #[arg(long, default_value_t = false)]
    list: bool,

    /// Do not try to validate against `opencode models` output; use slug directly
    #[arg(long, default_value_t = false)]
    no_validate: bool,

    /// Force picker backend: auto, fzf, dialoguer
    #[arg(long, value_enum, default_value_t = Backend::Auto)]
    backend: Backend,

    /// Generate shell completions and exit
    #[arg(long, value_enum)]
    completions: Option<Shell>,

    /// Extra args to forward on exec (e.g. --continue)
    #[arg(last = true)]
    extra: Vec<String>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Backend {
    Auto,
    Fzf,
    Dialoguer,
}

fn get_models_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home
        .join(".config")
        .join("usages_limit")
        .join("models.toml"))
}

fn slugify(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_dash = false;
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            // '-', whitespace, '_', and all other punctuation collapse to one '-'
            out.push('-');
            prev_dash = true;
        }
    }
    // trim leading/trailing dashes/dots
    out.trim_matches(|c| c == '-' || c == '.').to_string()
}

fn load_models() -> Result<Vec<ModelUsage>> {
    let path = get_models_path()?;
    if !path.exists() {
        anyhow::bail!("models.toml not found at {}", path.display());
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let data: UsageData =
        toml::from_str(&content).context("Failed to parse models.toml (expected UsageData)")?;
    if data.models.is_empty() {
        anyhow::bail!("No model entries in {}", path.display());
    }
    // pick latest date key (lexicographically max, YYYY-MM-DD)
    let latest_key = data.models.keys().max().unwrap().clone();
    let mut v = data.models.get(&latest_key).unwrap().clone();
    // already sorted by requests_per_5h desc in scraper, but ensure it
    // (Unlimited/None ranks highest so it sorts first)
    let key = |m: &ModelUsage| m.requests_per_5h.unwrap_or(u64::MAX);
    v.sort_by_key(|b| std::cmp::Reverse(key(b)));
    Ok(v)
}

fn valid_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("opencode-model-picker").join("valid_models.txt"))
}

fn load_valid_cache_with_age() -> Option<(HashSet<String>, Duration)> {
    let path = valid_cache_path()?;
    let meta = fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::from_secs(0));
    let content = fs::read_to_string(&path).ok()?;
    let set: HashSet<String> = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if set.is_empty() {
        return None;
    }
    Some((set, age))
}

fn spawn_detached_cache_refresh() {
    let Some(path) = valid_cache_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let path_str = path.to_string_lossy().to_string();
    let _ = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "opencode models > \"{}.tmp\" 2>/dev/null && mv \"{}.tmp\" \"{}\" 2>/dev/null || true",
            path_str, path_str, path_str
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn fetch_valid_models() -> HashSet<String> {
    const TTL_FRESH: Duration = Duration::from_secs(6 * 3600);

    if let Some((cached, age)) = load_valid_cache_with_age() {
        if age < TTL_FRESH {
            return cached;
        }
        spawn_detached_cache_refresh();
        return cached;
    }

    spawn_detached_cache_refresh();
    HashSet::new()
}

#[derive(Debug, Clone)]
struct Entry {
    display_name: String,
    requests: Option<u64>,
    model_id: String,
}

fn format_requests(n: Option<u64>) -> String {
    match n {
        None => "∞".to_string(),
        Some(v) if v >= 1000 => format!("{:.1}k", v as f64 / 1000.0),
        Some(v) => v.to_string(),
    }
}

fn build_entries(models: &[ModelUsage], valid: &HashSet<String>, no_validate: bool) -> Vec<Entry> {
    let mut entries = Vec::new();
    for m in models {
        let slug = slugify(&m.name);
        let candidates: Vec<String> = vec![
            format!("opencode-go/{}", slug),
            format!("opencode/{}-free", slug),
            // Some zen free models are without -free? like opencode/mimo-v2.5-free is the only; but also check plain opencode/<slug>
            format!("opencode/{}", slug),
        ];
        let mut added = HashSet::new();
        for cand in candidates {
            if added.contains(&cand) {
                continue;
            }
            if no_validate || valid.is_empty() {
                // only add the primary opencode-go candidate to avoid duplicates
                if cand.starts_with("opencode-go/") {
                    entries.push(Entry {
                        display_name: m.name.clone(),
                        requests: m.requests_per_5h,
                        model_id: cand.clone(),
                    });
                    added.insert(cand);
                    break; // only first
                }
                continue;
            }
            if valid.contains(&cand) {
                entries.push(Entry {
                    display_name: m.name.clone(),
                    requests: m.requests_per_5h,
                    model_id: cand.clone(),
                });
                added.insert(cand);
            }
        }
        // If validation on but nothing matched and no_validate false, fallback to opencode-go slug
        if !no_validate && !valid.is_empty() && added.is_empty() {
            let fallback = format!("opencode-go/{}", slug);
            // only if not already added and maybe model is genuinely new
            entries.push(Entry {
                display_name: m.name.clone(),
                requests: m.requests_per_5h,
                model_id: fallback,
            });
        }
        // When no_validate and valid empty, we already added primary
        if no_validate && valid.is_empty() && added.is_empty() {
            // shouldn't happen
        }
    }
    entries
}

fn fzf_available() -> bool {
    Command::new("fzf")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pick_with_fzf(entries: &[Entry]) -> Result<Option<String>> {
    // Build display lines: "Display Name  │ 45.3k/5h │ model-id"
    // Use tab or │ as separator; make it easy to extract last column.
    // We'll include ansi colors for nicer look if terminal supports.
    let mut input = String::new();
    // compute max name width for alignment
    let max_name = entries
        .iter()
        .map(|e| e.display_name.len())
        .max()
        .unwrap_or(20)
        .clamp(18, 38);
    for e in entries {
        let req = format_requests(e.requests);
        // pad name
        let padded = format!("{:<width$}", e.display_name, width = max_name);
        // Color: name bright, req dim, model cyan
        // Use ansi only if stdout is tty – fzf will pass through ansi
        // We'll embed ansi codes; fzf --ansi will render them
        let line = format!(
            "{}  \x1b[2m│\x1b[0m {:>7}/5h  \x1b[2m│\x1b[0m \x1b[36m{}\x1b[0m",
            padded, req, e.model_id
        );
        input.push_str(&line);
        input.push('\n');
    }

    let mut child = Command::new("fzf")
        .args([
            "--height",
            "60%",
            "--reverse",
            "--ansi",
            "--prompt",
            "Model ❯ ",
            "--header",
            "↑/↓ select • enter confirm • esc cancel  •  Alt+M picker (reads ~/.config/usages_limit/models.toml)",
            "--with-nth",
            "1..",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn fzf")?;

    {
        let stdin = child.stdin.as_mut().context("Failed to open fzf stdin")?;
        stdin
            .write_all(input.as_bytes())
            .context("Failed to write to fzf")?;
    }

    let output = child.wait_with_output().context("fzf wait failed")?;
    if !output.status.success() {
        // cancelled (130) or error
        return Ok(None);
    }
    let sel = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sel.is_empty() {
        return Ok(None);
    }
    // sel contains ansi; strip ansi to extract model id (last field after │)
    let stripped = strip_ansi(&sel);
    // split by │ and take last
    let parts: Vec<&str> = stripped.split('│').collect();
    let last = parts.last().unwrap_or(&stripped.as_str()).trim();
    // last is model id
    if last.is_empty() {
        return Ok(None);
    }
    // validate it looks like a model id (contains '/')
    if last.contains('/') {
        Ok(Some(last.to_string()))
    } else {
        Ok(Some(stripped.trim().to_string()))
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            // consume until letter
            for ch in chars.by_ref() {
                if ch.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn pick_with_dialoguer(entries: &[Entry]) -> Result<Option<String>> {
    use dialoguer::{theme::ColorfulTheme, Select};
    let theme = ColorfulTheme::default();
    let items: Vec<String> = entries
        .iter()
        .map(|e| {
            let req = format_requests(e.requests);
            format!("{}  ({}/5h)  — {}", e.display_name, req, e.model_id)
        })
        .collect();
    let sel = Select::with_theme(&theme)
        .with_prompt("Select opencode model (ESC to cancel)")
        .items(&items)
        .default(0)
        .interact_opt()
        .context("dialoguer failed")?;
    Ok(sel.map(|idx| entries[idx].model_id.clone()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "opencode-model-picker", &mut io::stdout());
        return Ok(());
    }

    let models = load_models()?;
    let valid = if cli.no_validate {
        HashSet::new()
    } else {
        fetch_valid_models()
    };

    let entries = build_entries(&models, &valid, cli.no_validate);

    if entries.is_empty() {
        anyhow::bail!("No entries built — check models.toml and `opencode models`");
    }

    if cli.list {
        for e in &entries {
            println!(
                "{:<38} {:>7}/5h  {}",
                e.display_name,
                format_requests(e.requests),
                e.model_id
            );
        }
        return Ok(());
    }

    let choice: Option<String> = match cli.backend {
        Backend::Fzf => pick_with_fzf(&entries)?,
        Backend::Dialoguer => pick_with_dialoguer(&entries)?,
        Backend::Auto => {
            if fzf_available() {
                match pick_with_fzf(&entries) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("fzf failed ({e}), falling back to dialoguer...");
                        pick_with_dialoguer(&entries)?
                    }
                }
            } else {
                pick_with_dialoguer(&entries)?
            }
        }
    };

    let Some(model_id) = choice else {
        // cancelled
        std::process::exit(130);
    };

    if cli.exec {
        // exec opencode --model <id> plus extra args
        let mut args = vec!["--model".to_string(), model_id.clone()];
        args.extend(cli.extra.clone());
        // Use exec to replace current process
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let err = Command::new("opencode").args(&args).exec();
            // if exec returns, it's an error
            anyhow::bail!("exec failed: {}", err);
        }
        #[cfg(not(unix))]
        {
            let status = Command::new("opencode").args(&args).status()?;
            std::process::exit(status.code().unwrap_or(1));
        }
    } else {
        // default: print to stdout for shell widget to capture
        println!("{}", model_id);
        // also if extra args provided and user expects immediate run? just print
        if !cli.extra.is_empty() {
            eprintln!(
                "Note: extra args {:?} ignored without --exec. Use --exec to run opencode.",
                cli.extra
            );
        }
    }

    Ok(())
}
