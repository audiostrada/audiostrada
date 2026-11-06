//! AI assistants that can drive AudioStrada through MCP: where each one keeps its list of MCP
//! servers, and adding AudioStrada to it (or taking it out). Each starts `audiostrada mcp
//! --connect <port>`, which reaches this window through the control channel.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The control channel's port unless changed.
pub const DEFAULT_PORT: u16 = 47_700;
const NAME: &str = "audiostrada";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    ClaudeDesktop,
    ClaudeCode,
    Cursor,
    GeminiCli,
}

impl Client {
    pub const ALL: [Client; 4] = [
        Client::ClaudeDesktop,
        Client::ClaudeCode,
        Client::Cursor,
        Client::GeminiCli,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeDesktop => "Claude Desktop",
            Client::ClaudeCode => "Claude Code",
            Client::Cursor => "Cursor",
            Client::GeminiCli => "Gemini CLI",
        }
    }

    /// Its file of MCP servers; Claude Code keeps its own, changed through its `claude` command.
    pub fn config_file(self) -> Option<PathBuf> {
        let home = dirs::home_dir();
        match self {
            Client::ClaudeDesktop => {
                dirs::config_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"))
            }
            Client::ClaudeCode => None,
            Client::Cursor => home.map(|h| h.join(".cursor").join("mcp.json")),
            Client::GeminiCli => home.map(|h| h.join(".gemini").join("settings.json")),
        }
    }

    /// Whether the assistant seems installed on this computer.
    pub fn found(self) -> bool {
        match self {
            Client::ClaudeCode => claude_command().is_some(),
            other => other
                .config_file()
                .and_then(|f| f.parent().map(Path::exists))
                .unwrap_or(false),
        }
    }
}

/// How the assistant starts AudioStrada's MCP server: this executable (the AppImage itself
/// when run from one, not its temporary mount).
pub fn command(port: u16) -> (String, Vec<String>) {
    let exe = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("audiostrada"));
    (
        exe.display().to_string(),
        vec!["mcp".into(), "--connect".into(), port.to_string()],
    )
}

/// The entry of a file of MCP servers, as most assistants read it.
pub fn snippet(port: u16) -> String {
    let (command, args) = command(port);
    let value = json!({"mcpServers": {NAME: {"command": command, "args": args}}});
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// The line that adds AudioStrada to Claude Code.
pub fn claude_code_line(port: u16) -> String {
    let (command, args) = command(port);
    let quote = |s: &str| {
        if s.contains(' ') {
            format!("\"{s}\"")
        } else {
            s.to_owned()
        }
    };
    format!(
        "claude mcp add --scope user {NAME} -- {} {}",
        quote(&command),
        args.join(" ")
    )
}

fn claude_command() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.cmd", "claude.exe", "claude"]
    } else {
        &["claude"]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // Graphical sessions often miss the folders an installer added to the shell's PATH.
    if let Some(home) = dirs::home_dir() {
        dirs.extend([
            home.join(".local/bin"),
            home.join(".claude/local"),
            home.join(".npm-global/bin"),
        ]);
    }
    dirs.iter()
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| runnable(p))
}

/// A file that can be run: as the shell does, a leftover without the execute bit is skipped.
fn runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// Whether AudioStrada is in the assistant's servers, and for this port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Added,
    /// Added, with another port or another executable.
    Outdated,
    NotAdded,
    Unknown(String),
}

pub fn state(client: Client, port: u16) -> State {
    match client {
        Client::ClaudeCode => {
            let Some(claude) = claude_command() else {
                return State::NotAdded;
            };
            match std::process::Command::new(claude)
                .args(["mcp", "get", NAME])
                .output()
            {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    if text.contains(&format!("--connect {port}")) {
                        State::Added
                    } else {
                        State::Outdated
                    }
                }
                Ok(_) => State::NotAdded,
                Err(e) => State::Unknown(e.to_string()),
            }
        }
        other => match other.config_file() {
            Some(file) => file_state(&file, port),
            None => State::NotAdded,
        },
    }
}

pub fn add(client: Client, port: u16) -> Result<(), String> {
    match client {
        Client::ClaudeCode => {
            let claude = claude_command()
                .ok_or("the `claude` command was not found: copy the line below into a terminal")?;
            // Replaced if there: the port or the executable may have changed.
            let _ = std::process::Command::new(&claude)
                .args(["mcp", "remove", "--scope", "user", NAME])
                .output();
            let (command, args) = command(port);
            let out = std::process::Command::new(&claude)
                .args(["mcp", "add", "--scope", "user", NAME, "--", &command])
                .args(&args)
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
            }
        }
        other => add_to_file(&other.config_file().ok_or("no configuration folder")?, port),
    }
}

pub fn remove(client: Client) -> Result<(), String> {
    match client {
        Client::ClaudeCode => {
            let claude = claude_command().ok_or("the `claude` command was not found")?;
            let out = std::process::Command::new(claude)
                .args(["mcp", "remove", "--scope", "user", NAME])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
            }
        }
        other => remove_from_file(&other.config_file().ok_or("no configuration folder")?),
    }
}

fn read(file: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(file) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON ({e}): left as it is", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

fn write(file: &Path, value: &Value) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // The assistant's file as it was, the first time AudioStrada changes it.
    let backup = file.with_extension("json.audiostrada-backup");
    if file.exists() && !backup.exists() {
        std::fs::copy(file, &backup).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(file, text + "\n").map_err(|e| e.to_string())
}

pub fn file_state(file: &Path, port: u16) -> State {
    match read(file) {
        Ok(v) => match v.get("mcpServers").and_then(|s| s.get(NAME)) {
            None => State::NotAdded,
            Some(entry) => {
                let (command, args) = command(port);
                if entry.get("command").and_then(Value::as_str) == Some(command.as_str())
                    && entry.get("args") == Some(&json!(args))
                {
                    State::Added
                } else {
                    State::Outdated
                }
            }
        },
        Err(e) => State::Unknown(e),
    }
}

pub fn add_to_file(file: &Path, port: u16) -> Result<(), String> {
    let mut v = read(file)?;
    let object = v.as_object_mut().ok_or("the file holds no JSON object")?;
    let servers = object.entry("mcpServers").or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or("mcpServers is not an object")?;
    let (command, args) = command(port);
    servers.insert(NAME.into(), json!({"command": command, "args": args}));
    write(file, &v)
}

pub fn remove_from_file(file: &Path) -> Result<(), String> {
    let mut v = read(file)?;
    let removed = v
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .and_then(|s| s.remove(NAME))
        .is_some();
    if removed { write(file, &v) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_keeps_what_the_file_had() {
        let dir = std::env::temp_dir().join(format!("audiostrada-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("claude_desktop_config.json");
        assert_eq!(file_state(&file, 47_700), State::NotAdded);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &file,
            r#"{"theme": "dark", "mcpServers": {"other": {"command": "x"}}}"#,
        )
        .unwrap();
        add_to_file(&file, 47_700).unwrap();
        assert_eq!(file_state(&file, 47_700), State::Added);
        assert_eq!(file_state(&file, 47_701), State::Outdated);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            v["mcpServers"]["audiostrada"]["args"],
            json!(["mcp", "--connect", "47700"])
        );
        assert!(
            dir.join("claude_desktop_config.json.audiostrada-backup")
                .exists()
        );
        remove_from_file(&file).unwrap();
        assert_eq!(file_state(&file, 47_700), State::NotAdded);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        // A file that is not JSON is never overwritten.
        std::fs::write(&file, "{ broken").unwrap();
        assert!(add_to_file(&file, 47_700).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ broken");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
