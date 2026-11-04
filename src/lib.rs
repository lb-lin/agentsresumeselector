//! agents: pick a Claude Code, Codex or Pi conversation, from any folder, and resume it there.
mod wide;

use memchr::{memchr, memmem, memrchr};
use memmap2::Mmap;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const HELP: &str = "agents: pick a Claude Code, Codex or Pi conversation, from any folder, and resume it there.

  agents                          the list you looked at last (all agents at first)
  agents --all | --claude | --codex | --pi   start on that list (←/→ switch in the list)
  agents --archived               the Codex conversations you archived
  agents <words>                  start with that filter
  agents .                        only conversations in this folder (and below)
  agents new [claude|codex|pi] [dir]   a new conversation (no dir: pick one)
  agents update                   install the newest version
  agents ... -- <args>            anything after `--` goes to the agent

https://github.com/lb-lin/agentsresumeselector

In the list: the switcher under the input shows which list you are on (all / Claude / Codex /
Pi). With nothing typed, → moves to the next list and ← back; once you type, ←/→ move in the search
text again. Tab / Shift-Tab switch lists either way. Enter resumes in the conversation's own folder,
Ctrl-O forks it (a copy to go on from, the original stays as it was), Ctrl-N starts a new one
with the same agent in that folder. When the agent exits you are back where you were.
Ctrl-X archives a Codex conversation with Codex's own archive: it leaves the lists, and the
已归档 (archived) list brings it back (Ctrl-X there, or Enter to go on with it). Claude Code and Pi
have no archive, and agents makes none. Ctrl-D deletes a conversation for good, once you answer y. ● marks a conversation a running agent holds: it can be forked, not resumed, archived
or deleted.";

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "lb-lin/agentsresumeselector";
pub const AGENTS: [&str; 3] = ["claude", "codex", "pi"];
const MODES: [&str; 5] = ["all", "claude", "codex", "pi", "archived"];
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const FZF_SWITCH: (u32, u32) = (0, 46); // the transform action, which switching lists needs
const FZF_WITH_SHELL: (u32, u32) = (0, 51); // --with-shell: the arrows' check runs in sh, whatever $SHELL is
const FZF_FOOTER: (u32, u32) = (0, 65); // --footer: key help under the list, the switcher alone above it
const NEED_FZF: &str = "需要 fzf：重新运行安装命令，或从 https://github.com/junegunn/fzf 安装";
const SWITCH_HELP: &str = "←/→/Tab 切换列表 · ";
const ACTION_HELP: &str = "Enter 恢复 · Ctrl-O 分支 · Ctrl-N 同目录新建 · ";
const TIDY_HELP: &str = "Ctrl-X 归档（Codex）· Ctrl-D 删除 · ";
const TAIL_HELP: &str = "Ctrl-/ 预览 · ● 正被占用";
const ARCHIVED_HELP: &str = "Codex 归档的对话：Enter 取回并接着聊 · Ctrl-X 放回列表 · Ctrl-D 永久删除";

fn name_of(agent: &str) -> &'static str {
    match agent {
        "claude" => "Claude",
        "codex" => "Codex",
        _ => "Pi",
    }
}

fn color_of(agent: &str) -> &'static str {
    match agent {
        "claude" => "\x1b[38;5;173m",
        "codex" => "\x1b[38;5;75m",
        "pi" => "\x1b[38;5;114m",
        _ => "",
    }
}

fn label_of(mode: &str) -> &'static str {
    match mode {
        "all" => "全部",
        "archived" => "已归档",
        _ => name_of(mode),
    }
}

/// The agent's name as the 'static string the rows carry; None for anything that is no agent.
fn agent_named(agent: &str) -> Option<&'static str> {
    AGENTS.iter().copied().find(|a| *a == agent)
}

fn say(text: &str) {
    let _ = writeln!(io::stdout(), "{text}");
}

fn note(text: &str) {
    let _ = writeln!(io::stderr(), "{text}");
}

fn die(text: &str) -> ! {
    note(text);
    std::process::exit(1)
}

// ---------- paths, as a shell or Python would treat them ----------

/// Whitespace as Python's `str.split` and `str.strip` count it: Unicode's, plus the four separators \x1c-\x1f.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        b.to_string()
    } else if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

fn dirname(path: &str) -> String {
    let Some(i) = path.rfind('/') else { return String::new() };
    let head = &path[..=i];
    if head.bytes().all(|b| b == b'/') {
        head.to_string()
    } else {
        head.trim_end_matches('/').to_string()
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let slashes = path.bytes().take_while(|b| *b == b'/').count();
    // POSIX keeps exactly two leading slashes; one or three and more become one
    let lead = match slashes {
        0 => "",
        2 => "//",
        _ => "/",
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if lead.is_empty() {
                    parts.push(part);
                }
            }
            _ => parts.push(part),
        }
    }
    let joined = format!("{lead}{}", parts.join("/"));
    if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

fn getcwd() -> Option<String> {
    std::env::current_dir().ok().map(|d| d.to_string_lossy().into_owned())
}

fn abspath(path: &str) -> String {
    if path.starts_with('/') {
        normpath(path)
    } else {
        normpath(&join(&getcwd().unwrap_or_default(), path))
    }
}

/// Resolves symlinks as far as the path exists; the rest is kept as written.
pub fn realpath(path: &str) -> String {
    let mut seen = HashMap::new();
    abspath(&join_real("", path, &mut seen).0)
}

fn join_real(base: &str, rest: &str, seen: &mut HashMap<String, Option<String>>) -> (String, bool) {
    let mut path = base.to_string();
    let mut rest = rest;
    if let Some(stripped) = rest.strip_prefix('/') {
        rest = stripped;
        path = "/".to_string();
    }
    while !rest.is_empty() {
        let (name, tail) = rest.split_once('/').unwrap_or((rest, ""));
        rest = tail;
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            if path.is_empty() {
                path = "..".to_string();
            } else {
                let head = dirname(&path);
                if basename(&path) == ".." {
                    path = join(&join(&head, ".."), "..");
                } else {
                    path = head;
                }
            }
            continue;
        }
        let candidate = join(&path, name);
        let is_link = fs::symlink_metadata(&candidate).map(|m| m.file_type().is_symlink()).unwrap_or(false);
        if !is_link {
            path = candidate;
            continue;
        }
        if let Some(known) = seen.get(&candidate) {
            match known {
                Some(target) => {
                    path = target.clone();
                    continue;
                }
                None => return (join(&candidate, rest), false), // a loop: stop resolving here
            }
        }
        seen.insert(candidate.clone(), None);
        let target = fs::read_link(&candidate).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
        let (resolved, ok) = join_real(&path, &target, seen);
        path = resolved;
        if !ok {
            return (join(&path, rest), false);
        }
        seen.insert(candidate, Some(path.clone()));
    }
    (path, true)
}

fn c_string(ptr: *const libc::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: a non-null pw_dir from getpwuid/getpwnam is a NUL-terminated string
    Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
}

fn passwd_home(user: Option<&str>) -> Option<String> {
    // SAFETY: the record libc returns is read at once, before any other passwd call
    unsafe {
        let record = match user {
            Some(name) => libc::getpwnam(CString::new(name).ok()?.as_ptr()),
            None => libc::getpwuid(libc::getuid()),
        };
        if record.is_null() {
            None
        } else {
            c_string((*record).pw_dir)
        }
    }
}

/// `~` and `~user` at the start of a path; a user that does not exist leaves the path as it is.
fn expanduser(path: &str, home_env: Option<&str>) -> String {
    if !path.starts_with('~') {
        return path.to_string();
    }
    let end = path[1..].find('/').map_or(path.len(), |i| i + 1);
    let home = if end == 1 {
        home_env.map(str::to_string).or_else(|| passwd_home(None))
    } else {
        passwd_home(Some(&path[1..end]))
    };
    let Some(home) = home else { return path.to_string() };
    let out = format!("{}{}", home.trim_end_matches('/'), &path[end..]);
    if out.is_empty() {
        "/".to_string()
    } else {
        out
    }
}

fn executable(path: &str) -> bool {
    let Ok(c) = CString::new(path.as_bytes()) else { return false };
    // SAFETY: c is a valid NUL-terminated string for the length of the call
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

fn is_dir(path: &str) -> bool {
    Path::new(path).is_dir()
}

fn quote(text: &str) -> String {
    let safe = |b: u8| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b);
    if !text.is_empty() && text.bytes().all(safe) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\"'\"'"))
    }
}

/// The running program's own path.
fn own_path() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "agents".to_string())
}

/// The fzf installed beside this program (the installer's, maybe newer than one on PATH), else PATH's.
fn fzf_bin() -> String {
    let beside = join(&dirname(&realpath(&own_path())), "fzf");
    if executable(&beside) {
        beside
    } else {
        "fzf".to_string()
    }
}

/// Runs a command with stdout captured, gives up after `limit`. None when it cannot start or takes too long.
fn output_within(mut command: Command, limit: Duration) -> Option<String> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (done, result) = mpsc::channel();
    thread::spawn(move || {
        let mut got = Vec::new();
        let _ = stdout.read_to_end(&mut got);
        let _ = done.send(got);
    });
    match result.recv_timeout(limit) {
        Ok(got) => {
            let _ = child.wait();
            Some(String::from_utf8_lossy(&got).into_owned())
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

// ---------- reading the transcripts ----------

/// The same key/value written with a space after the colon (the default style of many JSON writers).
fn spaced(needle: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(needle);
    text.replace("\":\"", "\": \"").replace("\":{", "\": {").into_bytes()
}

/// The needle as this file writes it (compact or spaced); None when it holds neither.
fn style(buf: &[u8], needle: &[u8]) -> Option<Vec<u8>> {
    if memmem::find(buf, needle).is_some() {
        return Some(needle.to_vec());
    }
    let other = spaced(needle);
    (other != needle && memmem::find(buf, &other).is_some()).then_some(other)
}

/// A lone surrogate escape, as JavaScript writes a string cut mid-emoji, becomes `?`: strict JSON
/// parsers refuse it, and it should not cost the whole line.
fn tame_surrogates(raw: &[u8]) -> Vec<u8> {
    let hex = |at: usize| -> Option<u32> {
        let digits = raw.get(at..at + 4)?;
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    };
    let unit_at = |at: usize| -> Option<u32> {
        if raw.get(at) == Some(&b'\\') && raw.get(at + 1) == Some(&b'u') {
            hex(at + 2)
        } else {
            None
        }
    };
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != b'\\' {
            out.push(raw[i]);
            i += 1;
            continue;
        }
        match unit_at(i) {
            Some(0xD800..=0xDBFF) if matches!(unit_at(i + 6), Some(0xDC00..=0xDFFF)) => {
                out.extend_from_slice(&raw[i..i + 12]);
                i += 12;
            }
            Some(0xD800..=0xDFFF) => {
                out.push(b'?');
                i += 6;
            }
            _ => {
                let end = (i + 2).min(raw.len());
                out.extend_from_slice(&raw[i..end]);
                i = end;
            }
        }
    }
    out
}

/// A JSON object from bytes; None for anything else, broken or too deep.
pub fn parse(raw: &[u8]) -> Option<Value> {
    let raw = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw);
    let value: Value = match serde_json::from_slice(raw) {
        Ok(value) => value,
        Err(_) if memmem::find(raw, b"\\u").is_some() => serde_json::from_slice(&tame_surrogates(raw)).ok()?,
        Err(_) => return None,
    };
    value.is_object().then_some(value)
}

fn line_bounds(buf: &[u8], at: usize) -> (usize, usize) {
    let start = memrchr(b'\n', &buf[..at]).map_or(0, |i| i + 1);
    let end = memchr(b'\n', &buf[at..]).map_or(buf.len(), |i| at + i);
    (start, end)
}

fn line_at(buf: &[u8], at: usize) -> Option<Value> {
    let (start, end) = line_bounds(buf, at);
    parse(&buf[start..end])
}

fn first_line(buf: &[u8]) -> Option<Value> {
    line_at(buf, 0)
}

fn last_of(buf: &[u8], needle: &[u8], field: &str) -> Option<String> {
    let needle = style(buf, needle)?;
    let d = line_at(buf, memmem::rfind(buf, &needle)?)?;
    let value = d.get(field)?.as_str()?;
    (!value.trim_matches(is_space).is_empty()).then(|| value.to_string())
}

// What agents put into the person's turn that the person did not type: command echoes, system
// notes, injected context, image markers. A prompt starting with any other "<" is the person's.
const INJECTED: [&str; 20] = [
    "<command-name", "<command-message", "<command-args", "<local-command-", "<task-notification",
    "<system-reminder", "<user-prompt-submit-hook", "<ide_selection", "<ide_opened_file", "<bash-input",
    "<bash-stdout", "<bash-stderr", "<environment_context", "<user_instructions", "<turn_aborted",
    "<image name=", "</image>", "<INSTRUCTIONS>", "# AGENTS.md instructions", "[Request interrupted",
];

/// Text a person typed, not a command echo, a system note or injected context.
pub fn typed(text: Option<&str>) -> bool {
    let Some(text) = text else { return false };
    let text = text.trim_start_matches(is_space);
    !text.is_empty() && !INJECTED.iter().any(|mark| text.starts_with(mark))
}

/// What a message says: a string, or the first typed text among its {type: text} blocks.
fn blocks_text(content: Option<&Value>) -> Option<&str> {
    match content? {
        Value::String(text) => Some(text),
        Value::Array(blocks) => {
            let mut first = None;
            for block in blocks {
                if !matches!(block.get("type").and_then(Value::as_str), Some("text" | "input_text" | "output_text")) {
                    continue;
                }
                let Some(text) = block.get("text").and_then(Value::as_str) else { continue };
                if typed(Some(text)) {
                    return Some(text);
                }
                first = first.or(Some(text));
            }
            first
        }
        _ => None,
    }
}

/// Who said something (true: the person) and what, from one transcript line; None when it is no message.
type Said = for<'a> fn(&'a Value) -> Option<(bool, Option<&'a str>)>;

/// The first thing the person typed: the first `needle` line whose text `said` returns as typed.
/// Tool results ride on the person's turn too (Claude); they are skipped unread and not counted.
fn first_typed(buf: &[u8], needle: &[u8], said: Said) -> Option<String> {
    const LIMIT: usize = 300;
    let needle = style(buf, needle)?;
    let finder = memmem::Finder::new(&needle);
    let (mut pos, mut looked) = (0, 0);
    while looked < LIMIT {
        let at = pos + finder.find(buf.get(pos..)?)?;
        let (start, end) = line_bounds(buf, at);
        pos = end + 1;
        let line = &buf[start..end];
        if memmem::find(line, b"\"tool_result\"").is_some() {
            continue;
        }
        looked += 1;
        if let Some(d) = parse(line) {
            if let Some((true, text)) = said(&d) {
                if typed(text) {
                    return text.map(str::to_string);
                }
            }
        }
    }
    None
}

fn mapped(path: &str) -> Option<Mmap> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() == 0 {
        return None;
    }
    // SAFETY: the map is read only and lives no longer than the reading of this one file
    unsafe { Mmap::map(&file) }.ok()
}

/// The names in a folder: (files, real subfolders). A symlink to a folder is in neither; it is not followed.
fn scan(folder: &str) -> (Vec<String>, Vec<String>) {
    let (mut files, mut folders) = (Vec::new(), Vec::new());
    let Ok(entries) = fs::read_dir(folder) else { return (files, folders) };
    for entry in entries.flatten() {
        // a name that is not text cannot ride in fzf's fields
        let Ok(name) = entry.file_name().into_string() else { continue };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            folders.push(name);
        } else if !(kind.is_symlink() && entry.path().is_dir()) {
            files.push(name);
        }
    }
    (files, folders)
}

/// Every file under `root` whose name `accept` takes, until `visit` returns true.
fn walk(root: &str, accept: &dyn Fn(&str) -> bool, visit: &mut dyn FnMut(String, &str) -> bool) -> bool {
    let (files, folders) = scan(root);
    for name in files.iter().filter(|name| accept(name)) {
        if visit(join(root, name), name) {
            return true;
        }
    }
    folders.iter().any(|folder| walk(&join(root, folder), accept, visit))
}

fn is_jsonl(name: &str) -> bool {
    name.ends_with(".jsonl")
}

/// A string that can ride in one of fzf's tab-separated fields of a one-line row.
fn text_field(value: Option<&Value>) -> Option<&str> {
    value?.as_str().filter(|text| plain_field(text))
}

fn plain_field(text: &str) -> bool {
    !text.is_empty() && !text.contains(['\t', '\n', '\r'])
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

// Claude Code: <claude home>/projects/<folder>/<session id>.jsonl

fn claude_said(d: &Value) -> Option<(bool, Option<&str>)> {
    let kind = d.get("type")?.as_str()?;
    // isCompactSummary: the summary Claude writes when it compacts the conversation, not something said
    if !matches!(kind, "user" | "assistant") || truthy(d.get("isMeta")) || truthy(d.get("isSidechain")) || truthy(d.get("isCompactSummary")) {
        return None;
    }
    let m = d.get("message").filter(|m| m.is_object())?;
    Some((kind == "user", blocks_text(m.get("content"))))
}

fn skip_space(text: &str) -> &str {
    text.trim_start_matches(is_space)
}

/// `<command-name> /x </command-name>`, and what the first `<command-args>…</command-args>` after it holds.
fn command_in(text: &str) -> Option<(&str, &str)> {
    const OPEN: &str = "<command-name>";
    const CLOSE: &str = "</command-name>";
    let mut from = 0;
    while let Some(i) = text[from..].find(OPEN) {
        let after = from + i + OPEN.len();
        from = after;
        let rest = skip_space(&text[after..]);
        let end = rest.find(|c: char| c == '<' || is_space(c)).unwrap_or(rest.len());
        if end == 0 {
            continue;
        }
        let Some(tail) = skip_space(&rest[end..]).strip_prefix(CLOSE) else { continue };
        let args = tail
            .find("<command-args>")
            .map(|j| &tail[j + "<command-args>".len()..])
            .and_then(|held| held.find("</command-args>").map(|k| &held[..k]))
            .unwrap_or("");
        return Some((&rest[..end], args));
    }
    None
}

/// A conversation started with a slash command and answered: named by that command.
fn claude_command_start(buf: &[u8]) -> Option<String> {
    let needle = style(buf, b"\"type\":\"assistant\"")?;
    let finder = memmem::Finder::new(&needle);
    let (mut pos, mut answered) = (0, false);
    for _ in 0..50 {
        let Some(at) = buf.get(pos..).and_then(|rest| finder.find(rest)).map(|i| pos + i) else { break };
        let (start, end) = line_bounds(buf, at);
        pos = end + 1;
        if let Some(d) = parse(&buf[start..end]) {
            if let Some(m) = d.get("message").filter(|m| m.is_object()) {
                if m.get("model").and_then(Value::as_str) != Some("<synthetic>") {
                    answered = true;
                    break;
                }
            }
        }
    }
    if !answered {
        return None;
    }
    let d = line_at(buf, memmem::find(buf, b"<command-name>")?)?;
    let m = d.get("message").filter(|m| m.is_object())?;
    let (name, args) = command_in(blocks_text(m.get("content"))?)?;
    let name = if name.starts_with('/') { name.to_string() } else { format!("/{name}") };
    Some(format!("{name} {}", args.trim_matches(is_space)).trim_matches(is_space).to_string())
}

/// The folder the conversation ran in, from the first line that names one (a broken line is passed over).
fn claude_cwd(buf: &[u8]) -> Option<String> {
    let needle = style(buf, b"\"cwd\":\"")?;
    let finder = memmem::Finder::new(&needle);
    let mut pos = 0;
    for _ in 0..20 {
        let at = pos + finder.find(buf.get(pos..)?)?;
        let (start, end) = line_bounds(buf, at);
        pos = end + 1;
        if let Some(d) = parse(&buf[start..end]) {
            if let Some(cwd) = text_field(d.get("cwd")) {
                return Some(cwd.to_string());
            }
        }
    }
    None
}

// Codex: <codex home>/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl, names in session_index.jsonl

fn codex_said(d: &Value) -> Option<(bool, Option<&str>)> {
    let p = d.get("payload").filter(|p| p.is_object())?;
    let role = p.get("role")?.as_str()?;
    if d.get("type")?.as_str()? != "response_item" || p.get("type")?.as_str()? != "message" || !matches!(role, "user" | "assistant") {
        return None;
    }
    Some((role == "user", blocks_text(p.get("content"))))
}

// Pi: <pi agent dir>/sessions/--<folder>--/<time>_<session id>.jsonl, a {"type":"session"} header first

fn pi_said(d: &Value) -> Option<(bool, Option<&str>)> {
    let m = d.get("message").filter(|m| m.is_object())?;
    let role = m.get("role")?.as_str()?;
    if d.get("type")?.as_str()? != "message" || !matches!(role, "user" | "assistant") {
        return None;
    }
    Some((role == "user", blocks_text(m.get("content"))))
}

fn said_by(agent: &str) -> Said {
    match agent {
        "claude" => claude_said,
        "codex" => codex_said,
        _ => pi_said,
    }
}

/// One line, no tabs: fzf's fields are tab-separated and a row is one line.
fn clean(text: &str) -> String {
    text.split(is_space).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

fn mtime_of(path: &str) -> f64 {
    fs::metadata(path).map_or(0.0, |m| m.mtime() as f64 + m.mtime_nsec() as f64 / 1e9)
}

#[derive(Debug, Clone)]
pub struct Conversation {
    pub agent: &'static str,
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub path: String,
    pub mtime: f64,
    pub prompt: String,
    /// In the archive, not on the agent's own lists.
    pub archived: bool,
}

fn conversation(agent: &'static str, id: &str, cwd: String, title: &str, path: String, prompt: &str) -> Conversation {
    let mtime = mtime_of(&path);
    Conversation { agent, id: clean(id), cwd, title: clean(title), path, mtime, prompt: clean(prompt), archived: false }
}

fn claude_conversation(path: String, name: &str) -> Option<Conversation> {
    let buf = mapped(&path)?;
    let cwd = claude_cwd(&buf)?;
    let prompt = first_typed(&buf, b"\"type\":\"user\"", claude_said).or_else(|| claude_command_start(&buf))?;
    let title = last_of(&buf, b"\"type\":\"custom-title\"", "customTitle")
        .or_else(|| last_of(&buf, b"\"type\":\"ai-title\"", "aiTitle"))
        .or_else(|| last_of(&buf, b"\"type\":\"agent-name\"", "agentName"))
        .unwrap_or_else(|| prompt.clone());
    Some(conversation("claude", name.strip_suffix(".jsonl")?, cwd, &title, path, &prompt))
}

fn codex_conversation(path: String, names: &HashMap<String, String>) -> Option<Conversation> {
    let buf = mapped(&path)?;
    let head = first_line(&buf)?;
    let meta = head.get("payload").filter(|m| m.is_object())?;
    // Codex's own helpers (the guardian reviewer and other sub-agents) are no one's conversation
    if meta.get("source").is_some_and(|s| s.get("subagent").is_some()) {
        return None;
    }
    let thread = text_field(meta.get("id"))?;
    let cwd = text_field(meta.get("cwd"))?;
    let prompt = first_typed(&buf, b"\"role\":\"user\"", codex_said)?;
    let title = names.get(thread).unwrap_or(&prompt);
    Some(conversation("codex", thread, cwd.to_string(), title, path, &prompt))
}

fn pi_conversation(path: String) -> Option<Conversation> {
    let buf = mapped(&path)?;
    let head = first_line(&buf)?;
    if head.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let ident = text_field(head.get("id"))?;
    let cwd = text_field(head.get("cwd"))?;
    let prompt = first_typed(&buf, b"\"role\":\"user\"", pi_said)?;
    let title = last_of(&buf, b"\"type\":\"session_info\"", "name").unwrap_or_else(|| prompt.clone());
    Some(conversation("pi", ident, cwd.to_string(), &title, path, &prompt))
}

/// Pieces of the system-wide state that the held checks read; tests hand in their own.
pub type CommandLine<'a> = &'a dyn Fn(i32) -> Vec<String>;

/// Whether pid runs, and, where its start time is known, is still the process that started then.
pub fn alive(pid: i64, started: Option<&str>) -> bool {
    let Ok(pid) = i32::try_from(pid) else { return false };
    // SAFETY: signal 0 only asks whether the process exists
    if unsafe { libc::kill(pid, 0) } != 0 && io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
        return false;
    }
    let Some(started) = started else { return true };
    let Ok(stat) = fs::read(format!("/proc/{pid}/stat")) else { return true };
    // the fields after the command name, which may hold spaces and parentheses; starttime is the 22nd
    let Some(close) = memrchr(b')', &stat) else { return true };
    let fields = String::from_utf8_lossy(stat.get(close + 2..).unwrap_or_default()).into_owned();
    fields.split_whitespace().nth(19).is_none_or(|field| field == started)
}

/// The arguments a process was started with.
pub fn command_line(pid: i32) -> Vec<String> {
    match fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(raw) => raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect(),
        Err(_) => Vec::new(),
    }
}

/// The command lines of running processes.
fn processes() -> Vec<Vec<String>> {
    if is_dir("/proc/self") {
        let Ok(entries) = fs::read_dir("/proc") else { return Vec::new() };
        return entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok()))
            .map(command_line)
            .filter(|argv| !argv.is_empty())
            .collect();
    }
    let mut ps = Command::new("ps");
    ps.args(["-axo", "command="]);
    let listing = output_within(ps, Duration::from_secs(5)).unwrap_or_default();
    listing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().map(str::to_string).collect())
        .collect()
}

/// Whether a command line is this agent (codex directly; pi through node or bun too).
fn runs(argv: &[String], agent: &str) -> bool {
    let head = &argv[..argv.len().min(3)];
    if agent == "codex" {
        return head.first().is_some_and(|a| basename(a) == "codex");
    }
    head.iter().any(|a| basename(a) == "pi" || a.contains("pi-coding-agent"))
}

// The id that resumes a conversation follows these words. A fork names its source too, but holds
// only the new conversation, so `codex fork <id>` and `pi --fork <id>` hold nothing listed.
fn opener(agent: &str, word: &str) -> bool {
    match agent {
        "codex" => word == "resume",
        _ => word == "--session" || word == "--session-id",
    }
}

pub fn held_by_command_line(rows: &[Conversation], agent: &str, procs: &[Vec<String>]) -> HashSet<String> {
    let ids: HashSet<&str> = rows.iter().filter(|r| r.agent == agent).map(|r| r.id.as_str()).collect();
    let mut held = HashSet::new();
    for argv in procs.iter().filter(|argv| runs(argv, agent)) {
        for pair in argv.windows(2) {
            if opener(agent, &pair[0]) && ids.contains(pair[1].as_str()) {
                held.insert(pair[1].clone());
            }
        }
        if agent == "pi" {
            for a in argv {
                if let Some((flag, value)) = a.split_once('=') {
                    if opener("pi", flag) && ids.contains(value) {
                        held.insert(value.to_string());
                    }
                }
            }
        }
    }
    held
}

/// The folders running Pi processes work in (Linux). Pi renames its process to `pi`, which wipes
/// its command line: which conversation it has open cannot be told, only where it runs.
pub fn pi_folders() -> Vec<String> {
    let Ok(entries) = fs::read_dir("/proc") else { return Vec::new() };
    let mut folders = Vec::new();
    for pid in entries.flatten().filter_map(|entry| entry.file_name().to_str().and_then(|name| name.parse::<i32>().ok())) {
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim_end() == "pi" || runs(&command_line(pid), "pi") {
            if let Ok(cwd) = fs::read_link(format!("/proc/{pid}/cwd")) {
                folders.push(cwd.to_string_lossy().into_owned());
            }
        }
    }
    folders
}

/// {inode: holder pids} of locks held (Linux: /proc/locks).
pub fn lock_holders(path: &str) -> HashMap<u64, HashSet<i32>> {
    let mut holders: HashMap<u64, HashSet<i32>> = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else { return holders };
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // "48: FLOCK  ADVISORY  WRITE 1009782 08:01:786440 0 EOF"; "->" marks a waiter
        if parts.len() > 5 && parts[1] != "->" {
            let inode = parts[5].rsplit(':').next().unwrap_or("");
            if let (Ok(pid), Ok(inode)) = (parts[4].parse::<i32>(), inode.parse::<u64>()) {
                holders.entry(inode).or_default().insert(pid);
            }
        }
    }
    holders
}

// ---------- what depends on the environment ----------

pub struct Ctx {
    pub home: String,
    home_env: Option<String>,
    pub claude_home: String,
    pub codex_home: String,
    pub pi_sessions: String,
    state: String,
    /// `agents .`: only this folder and below (travels to fzf's reloads in the environment).
    pub here: Option<String>,
    /// $PATH: where agents are looked for.
    pub path: Option<String>,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        Ctx::with(|name| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned()))
    }

    pub fn with(get: impl Fn(&str) -> Option<String>) -> Ctx {
        let set = |name: &str| get(name).filter(|v| !v.is_empty());
        let home_env = get("HOME");
        let home = expanduser("~", home_env.as_deref());
        let claude_home = set("CLAUDE_CONFIG_DIR").unwrap_or_else(|| join(&home, ".claude"));
        let codex_home = set("CODEX_HOME").unwrap_or_else(|| join(&home, ".codex"));
        let pi_home = set("PI_CODING_AGENT_DIR").unwrap_or_else(|| join(&join(&home, ".pi"), "agent"));
        let pi_sessions = set("PI_CODING_AGENT_SESSION_DIR").unwrap_or_else(|| {
            // an absolute sessionDir in Pi's settings, else <dir>/sessions
            let configured = fs::read(join(&pi_home, "settings.json"))
                .ok()
                .and_then(|raw| parse(&raw))
                .and_then(|settings| settings.get("sessionDir").and_then(Value::as_str).map(|dir| expanduser(dir, home_env.as_deref())));
            configured.filter(|dir| dir.starts_with('/')).unwrap_or_else(|| join(&pi_home, "sessions"))
        });
        let state_home = set("XDG_STATE_HOME").unwrap_or_else(|| join(&join(&home, ".local"), "state"));
        Ctx {
            state: join(&state_home, "agents"),
            here: set("AGENTS_HERE"),
            path: get("PATH"),
            home,
            home_env,
            claude_home,
            codex_home,
            pi_sessions,
        }
    }

    fn session_root(&self, agent: &str) -> String {
        match agent {
            "claude" => join(&self.claude_home, "projects"),
            "codex" => join(&self.codex_home, "sessions"),
            _ => self.pi_sessions.clone(),
        }
    }

    /// Where an agent keeps what it archived. Only Codex has an archive of its own; agents makes
    /// none for the others, so Claude Code's and Pi's conversations are never moved.
    pub fn archive_root(&self, agent: &str) -> Option<String> {
        (agent == "codex").then(|| join(&self.codex_home, "archived_sessions"))
    }

    fn root(&self, agent: &str, archived: bool) -> Option<String> {
        if archived {
            self.archive_root(agent)
        } else {
            Some(self.session_root(agent))
        }
    }

    /// One agent's conversations, on its lists or in the archive.
    fn read_agent(&self, agent: &'static str, archived: bool) -> Vec<Conversation> {
        let Some(root) = self.root(agent, archived) else { return Vec::new() };
        let mut out = Vec::new();
        match agent {
            "claude" => {
                // subagent transcripts live in <session>/subagents/; only the top level is a conversation
                for folder in scan(&root).1 {
                    let folder = join(&root, &folder);
                    for name in scan(&folder).0.iter().filter(|name| is_jsonl(name)) {
                        out.extend(claude_conversation(join(&folder, name), name));
                    }
                }
            }
            "codex" => {
                let names = self.codex_names();
                walk(&root, &|name| name.starts_with("rollout-") && is_jsonl(name), &mut |path, _| {
                    out.extend(codex_conversation(path, &names));
                    false
                });
            }
            _ => {
                walk(&root, &is_jsonl, &mut |path, _| {
                    out.extend(pi_conversation(path));
                    false
                });
            }
        }
        out.iter_mut().for_each(|r| r.archived = archived);
        out
    }

    /// Thread names Codex keeps in session_index.jsonl (later lines win).
    fn codex_names(&self) -> HashMap<String, String> {
        let mut names = HashMap::new();
        let Ok(raw) = fs::read(join(&self.codex_home, "session_index.jsonl")) else { return names };
        let text = String::from_utf8_lossy(&raw);
        for line in text.split('\n') {
            let Some(d) = parse(line.as_bytes()) else { continue };
            if let (Some(id), Some(name)) = (d.get("id").and_then(Value::as_str), d.get("thread_name").and_then(Value::as_str)) {
                if !name.trim_matches(is_space).is_empty() {
                    names.insert(id.to_string(), name.to_string());
                }
            }
        }
        names
    }

    pub fn conversations(&self, mode: &str) -> Vec<Conversation> {
        let mut rows = Vec::new();
        let archived = mode == "archived";
        for agent in AGENTS {
            if archived || mode == "all" || mode == agent {
                rows.extend(self.read_agent(agent, archived));
            }
        }
        // a file path with a tab or newline cannot ride in fzf's fields; such a conversation is left out
        rows.retain(|r| plain_field(&r.path));
        if let Some(here) = &self.here {
            // a folder may be named through a symlink (macOS's /var is /private/var): compare both spellings
            let stripped = here.trim_end_matches('/');
            let tops = [if stripped.is_empty() { "/".to_string() } else { stripped.to_string() }, realpath(here)];
            let inside = |cwd: &str| {
                tops.iter().any(|top| cwd == top || cwd.strip_prefix(top.trim_end_matches('/')).is_some_and(|rest| rest.starts_with('/')))
            };
            rows.retain(|r| inside(&r.cwd) || inside(&realpath(&r.cwd)));
        }
        rows.sort_by(|a, b| b.mtime.partial_cmp(&a.mtime).unwrap_or(std::cmp::Ordering::Equal));
        rows
    }

    pub fn held_claude(&self) -> HashSet<String> {
        let mut held = HashSet::new();
        let folder = join(&self.claude_home, "sessions");
        let Ok(entries) = fs::read_dir(&folder) else { return held };
        for entry in entries.flatten() {
            if !entry.file_name().as_bytes().ends_with(b".json") {
                continue;
            }
            let Some(record) = fs::read(entry.path()).ok().and_then(|raw| parse(&raw)) else { continue };
            let Some(session) = record.get("sessionId").and_then(Value::as_str) else { continue };
            let Some(pid) = record.get("pid").and_then(Value::as_i64).filter(|pid| *pid > 0) else { continue };
            let started = match record.get("procStart") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Number(n)) if n.is_i64() || n.is_u64() => Some(n.to_string()),
                _ => None,
            };
            if alive(pid, started.as_deref()) {
                held.insert(session.to_string());
            }
        }
        held
    }

    /// Threads a running codex writes: it locks thread-writer-locks/<id>.lock, or resumes the id by name.
    pub fn held_codex(&self, rows: &[Conversation], procs: &[Vec<String>], holders: &HashMap<u64, HashSet<i32>>, argv_of: CommandLine) -> HashSet<String> {
        let mut held = held_by_command_line(rows, "codex", procs);
        if holders.is_empty() {
            return held;
        }
        let locks = join(&self.codex_home, "thread-writer-locks");
        let Ok(entries) = fs::read_dir(&locks) else { return held };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            let Some(thread) = name.strip_suffix(".lock").filter(|_| !name.starts_with('.')) else { continue };
            let Ok(meta) = fs::metadata(entry.path()) else { continue };
            // an inode number alone may belong to a lock on another disk: the holder must be codex
            if holders.get(&meta.ino()).is_some_and(|pids| pids.iter().any(|pid| runs(&argv_of(*pid), "codex"))) {
                held.insert(thread.to_string());
            }
        }
        held
    }

    pub fn held(&self, rows: &[Conversation]) -> HashSet<String> {
        let procs = processes();
        let mut held = self.held_claude();
        held.extend(self.held_codex(rows, &procs, &lock_holders("/proc/locks"), &command_line));
        held.extend(held_by_command_line(rows, "pi", &procs));
        held
    }

    // ---------- the list ----------

    pub fn short(&self, path: &str) -> String {
        match path.strip_prefix(self.home.as_str()) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        }
    }

    pub fn installed(&self, agent: &str) -> bool {
        let Some(path) = self.search_path() else { return false };
        path.split(':').any(|folder| {
            let candidate = join(folder, agent);
            Path::new(&candidate).exists() && executable(&candidate) && !is_dir(&candidate)
        })
    }

    fn search_path(&self) -> Option<String> {
        match &self.path {
            Some(path) if path.is_empty() => None,
            Some(path) => Some(path.clone()),
            None => Some("/bin:/usr/bin".to_string()),
        }
    }

    /// An agent worth a list: installed here, or with conversation files left from before.
    pub fn present(&self, agent: &str) -> bool {
        self.installed(agent) || walk(&self.session_root(agent), &is_jsonl, &mut |_, _| true)
    }

    /// Whether anything is archived: the archived list is there only then.
    pub fn has_archived(&self) -> bool {
        AGENTS.iter().filter_map(|agent| self.archive_root(agent)).any(|root| walk(&root, &is_jsonl, &mut |_, _| true))
    }

    /// A list worth showing: the mixed one always, an agent's when it is present, the archive when it holds something.
    fn available(&self, mode: &str) -> bool {
        match mode {
            "all" => true,
            "archived" => self.has_archived(),
            agent => self.present(agent),
        }
    }

    pub fn rows_for(&self, mode: &str) -> Vec<String> {
        let rows = self.conversations(mode);
        let archive = mode == "archived";
        // nothing runs an archived conversation
        let busy = if archive { HashSet::new() } else { self.held(&rows) };
        let mut lines = Vec::with_capacity(rows.len() + 3);
        for agent in AGENTS {
            if (mode == "all" || mode == agent) && self.installed(agent) {
                lines.push(format!("new\t{agent}\t\t\t\t{}＋ 新建 {} 对话…{RESET}", color_of(agent), name_of(agent)));
            }
        }
        let now = now();
        for r in &rows {
            let mark = if busy.contains(&r.id) { "●" } else { " " };
            // on the mixed list every row names its agent; on one agent's list the prompt does
            let who = if mode == "all" || archive { format!("{} ", tag(r.agent)) } else { String::new() };
            let title = chars_upto(&r.title, 140);
            // the first thing said rides along (dim, mostly off screen) so it can be searched too
            let said = if !r.prompt.is_empty() && (r.prompt != r.title || title.len() < r.title.len()) {
                format!("  {DIM}· {}{RESET}", chars_upto(&r.prompt, 300))
            } else {
                String::new()
            };
            let shown = format!("{mark} {} {who}{DIM}{}{RESET} {title}{said}", pad(&ago_from(now, r.mtime), 8), pad(&self.short(&r.cwd), 26));
            let action = if r.archived { "archived" } else { "resume" };
            lines.push(format!("{action}\t{}\t{}\t{}\t{}\t{shown}", r.agent, r.id, r.cwd, r.path));
        }
        lines
    }

    pub fn remember(&self, mode: &str) {
        if fs::create_dir_all(&self.state).is_ok() {
            let _ = fs::write(join(&self.state, "mode"), mode);
        }
    }

    pub fn remembered(&self) -> String {
        let Ok(raw) = fs::read(join(&self.state, "mode")) else { return "all".to_string() };
        let Ok(text) = String::from_utf8(raw) else { return "all".to_string() };
        let mode = text.trim_matches(is_space);
        if MODES.contains(&mode) && self.available(mode) {
            mode.to_string()
        } else {
            "all".to_string()
        }
    }

    pub fn lists(&self) -> Vec<&'static str> {
        let mut order = vec!["all"];
        order.extend(AGENTS.iter().copied().filter(|agent| self.present(agent)));
        if self.has_archived() {
            order.push("archived");
        }
        order
    }

    /// The lists, the one shown highlighted in its agent's color: 全部  Claude  Codex  Pi.
    pub fn switcher(&self, mode: &str) -> String {
        let parts: Vec<String> = self
            .lists()
            .into_iter()
            .map(|m| {
                if m == mode {
                    format!("{}\x1b[1;7m {} {RESET}", color_of(m), label_of(m))
                } else {
                    format!("{DIM} {} {RESET}", label_of(m))
                }
            })
            .collect();
        let place = self.here.as_ref().map_or(String::new(), |here| format!("   {DIM}只看 {}{RESET}", self.short(here)));
        format!("{}{place}", parts.join(" "))
    }

    /// What sits under the input: the switcher, and the key help when there is no footer for it.
    pub fn header(&self, mode: &str, footer: bool) -> String {
        let mut top = self.switcher(mode);
        // the keys mean something else on the archived list: say so where the eye is
        if mode == "archived" {
            top += &format!("   {DIM}{ARCHIVED_HELP}{RESET}");
        }
        if !footer {
            top += &format!("   {DIM}{SWITCH_HELP}{ACTION_HELP}{TIDY_HELP}{TAIL_HELP}{RESET}");
        }
        top
    }

    /// fzf actions that show a list: its rows and the header for it.
    pub fn show(&self, mode: &str, footer: bool) -> String {
        // change-header: takes the rest of the line, so it comes last
        format!("reload({} --list {mode})+change-header:{}", me(), self.header(mode, footer))
    }

    /// fzf actions that leave the list as it is and say what went wrong under the switcher.
    fn warn(&self, footer: bool, text: &str) -> String {
        format!("change-header:{}   {RED}{text}{RESET}", self.header(&self.remembered(), footer))
    }

    /// → / ← (or Tab / Shift-Tab): the next (or previous) list, skipping absent agents. The list shown is
    /// the remembered one. An arrow with text typed moves in the search text instead.
    pub fn cycle(&self, step: i32, footer: bool, arrow: bool, query: Option<&str>) -> String {
        if arrow && query.is_some_and(|q| !q.is_empty()) {
            return if step > 0 { "forward-char" } else { "backward-char" }.to_string();
        }
        let order = self.lists();
        let now = self.remembered();
        let nxt = match order.iter().position(|m| *m == now) {
            Some(i) => order[(i as i32 + step).rem_euclid(order.len() as i32) as usize],
            None => "all",
        };
        self.remember(nxt);
        self.show(nxt, footer)
    }

    // ---------- archiving and deleting ----------

    /// Where a transcript of this agent's lives: (archived, its path below that root). None for any
    /// other file: archiving and deleting touch the agents' own transcripts and nothing else.
    fn place(&self, agent: &str, path: &str) -> Option<(bool, String)> {
        let path = normpath(path);
        if !path.starts_with('/') || !is_jsonl(&path) {
            return None;
        }
        for archived in [true, false] {
            let Some(root) = self.root(agent, archived).map(|root| normpath(&root)) else { continue };
            if let Some(rel) = path.strip_prefix(&format!("{}/", root.trim_end_matches('/'))) {
                return Some((archived, rel.to_string()));
            }
        }
        None
    }

    /// Why this conversation may not be archived or deleted now, if a running agent may hold it.
    fn holder(&self, agent: &'static str, id: &str, path: &str) -> Option<String> {
        let name = name_of(agent);
        if self.is_held(agent, id) {
            return Some(format!("这个对话正被另一个 {name} 进程占着"));
        }
        // a Pi working in the conversation's folder may be the one that has it open: deleting the file
        // under it would leave it writing a headless fragment where the transcript was
        if agent == "pi" {
            let cwd = self.read_one(agent, path).map(|c| realpath(&c.cwd))?;
            if pi_folders().iter().any(|folder| realpath(folder) == cwd) {
                return Some("这个目录里有 Pi 在运行，可能正开着这个对话".to_string());
            }
        }
        None
    }

    fn is_held(&self, agent: &'static str, id: &str) -> bool {
        let row = Conversation { agent, id: id.to_string(), cwd: String::new(), title: String::new(), path: String::new(), mtime: 0.0, prompt: String::new(), archived: false };
        self.held(std::slice::from_ref(&row)).contains(id)
    }

    fn codex(&self, args: &[&str]) -> Result<(), String> {
        if !self.installed("codex") {
            return Err("这台机器上没有 codex".to_string());
        }
        let out = Command::new("codex").args(args).stdin(Stdio::null()).output().map_err(|e| format!("运行不了 codex：{}", strerror(&e)))?;
        if out.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        let last = said.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("WARNING")).last().unwrap_or("");
        Err(format!("codex {} 出错：{last}", args[0]))
    }

    /// Takes a Codex conversation off the lists into Codex's archive (`to_archive`), or back.
    pub fn archive(&self, agent: &'static str, id: &str, path: &str, to_archive: bool) -> Result<(), String> {
        if agent != "codex" {
            return Err(format!("{} 自己没有归档，agents 也不替它做：不要了可以按 Ctrl-D 删除", name_of(agent)));
        }
        let (archived, _) = self.place(agent, path).ok_or_else(|| "这不是它的对话记录".to_string())?;
        if archived == to_archive {
            return Ok(());
        }
        // Codex keeps its own index of what is archived: let it do the moving
        self.codex(&[if to_archive { "archive" } else { "unarchive" }, id])
    }

    /// Deletes a conversation for good, on the lists or in the archive.
    pub fn delete(&self, agent: &'static str, id: &str, path: &str) -> Result<(), String> {
        let (archived, rel) = self.place(agent, path).ok_or_else(|| "这不是它的对话记录".to_string())?;
        if agent == "codex" {
            // agents has asked already; codex asks only where it has a terminal of its own
            return self.codex(&["delete", "--force", id]);
        }
        let file = join(&self.root(agent, archived).unwrap_or_default(), &rel);
        // the transcript, as Pi's own session picker deletes one
        fs::remove_file(&file).map_err(|e| format!("删不掉 {file}：{}", strerror(&e)))?;
        // Claude keeps a conversation's subagents and large tool results in a folder of the same name
        let folder = file.trim_end_matches(".jsonl");
        if agent == "claude" && is_dir(folder) {
            let _ = fs::remove_dir_all(folder);
        }
        Ok(())
    }

    /// Ctrl-X (fzf's transform): archive the row, or put an archived one back, and show the list again.
    pub fn archive_key(&self, action: &str, agent: &str, id: &str, path: &str, footer: bool) -> String {
        let Some(agent) = agent_named(agent) else { return String::new() };
        let to_archive = match action {
            "resume" => true,
            "archived" => false,
            _ => return String::new(), // a new-conversation row: nothing to archive
        };
        if agent != "codex" {
            return self.warn(footer, &format!("{} 自己没有归档，agents 也不替它做：不要了可以按 Ctrl-D 删除", name_of(agent)));
        }
        if let Some(why) = self.holder(agent, id, path).filter(|_| to_archive) {
            return self.warn(footer, &format!("{why}，不能归档：先在那边退出"));
        }
        if let Err(e) = self.archive(agent, id, path, to_archive) {
            return self.warn(footer, &format!("{}失败：{e}", if to_archive { "归档" } else { "放回" }));
        }
        // the archived list goes when its last conversation does
        let mode = self.remembered();
        self.remember(&mode);
        self.show(&mode, footer)
    }

    /// Ctrl-D (fzf's execute, on the terminal): ask, then delete for good.
    pub fn delete_key(&self, action: &str, agent: &str, id: &str, cwd: &str, path: &str) {
        let Some(agent) = agent_named(agent) else { return };
        if action != "resume" && action != "archived" {
            return;
        }
        let name = name_of(agent);
        if let Some(why) = self.holder(agent, id, path).filter(|_| action == "resume") {
            note(&format!("\n{why}，不能删除：先在那边退出。"));
            return pause();
        }
        let title = self.read_one(agent, path).map(|c| c.title).unwrap_or_default();
        note(&format!("\n永久删除这个 {name} 对话？删除后找不回来。\n\n  {} · {}\n", self.short(cwd), chars_upto(&title, 100)));
        let _ = write!(io::stderr(), "输入 y 确认，其他任意内容取消：");
        let _ = io::stderr().flush();
        let mut answer = String::new();
        let _ = io::stdin().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            return;
        }
        if let Err(e) = self.delete(agent, id, path) {
            note(&format!("删除失败：{e}"));
            pause();
        }
    }

    fn read_one(&self, agent: &str, path: &str) -> Option<Conversation> {
        match agent {
            "claude" => claude_conversation(path.to_string(), basename(path)),
            "codex" => codex_conversation(path.to_string(), &self.codex_names()),
            _ => pi_conversation(path.to_string()),
        }
    }

    /// A conversation's last few exchanges, for the side panel.
    pub fn preview(&self, agent: &str, path: &str, ident: &str, cwd: &str) {
        if !AGENTS.contains(&agent) {
            return;
        }
        let name = name_of(agent);
        if path.is_empty() {
            say(&format!("新开一个 {name} 对话：接下来选目录"));
            return;
        }
        let Ok(meta) = fs::metadata(path) else { return };
        let size = meta.len();
        say(&format!("{name} · {} · 更新于 {} · {ident}\n", self.short(cwd), ago_from(now(), mtime_of(path))));
        let said_of = said_by(agent);
        let mut window: u64 = 600_000;
        let mut said: Vec<String> = Vec::new();
        loop {
            let start = size.saturating_sub(window);
            let Some(raw) = read_from(path, start, size - start) else { return };
            // a window that starts mid-file starts mid-line
            for row in raw.split(|b| *b == b'\n').skip(usize::from(start > 0)) {
                let Some(d) = parse(row) else { continue };
                if let Some((is_person, text)) = said_of(&d) {
                    if typed(text) {
                        let who = if is_person { "你" } else { name };
                        let text = text.unwrap_or_default().trim_matches(is_space);
                        said.push(format!("── {who}\n{}", chars_upto(text, 700)));
                    }
                }
            }
            if !said.is_empty() || start == 0 {
                break;
            }
            window *= 4;
        }
        if said.is_empty() {
            say("（还没有对话）");
        } else {
            say(&said[said.len().saturating_sub(8)..].join("\n\n"));
        }
    }

    // ---------- starting the agent ----------

    fn run(&self, agent: &str, cwd: &str, argv: Vec<String>) {
        if !is_dir(cwd) {
            die(&format!("目录不在了：{cwd}"));
        }
        if !self.installed(agent) {
            die(&format!("这台机器上没有 {agent}"));
        }
        let shown: Vec<String> = argv.iter().map(|a| quote(a)).collect();
        note(&format!("→ {}  {}", self.short(cwd), shown.join(" ")));
        if std::env::var_os("AGENTS_DRY_RUN").is_some_and(|v| !v.is_empty()) {
            say(&format!("{{\"cwd\": {}, \"argv\": {}}}", Value::from(cwd), Value::from(argv)));
            return;
        }
        if let Err(error) = std::env::set_current_dir(cwd) {
            die(&format!("进不去这个目录：{cwd}（{}）", strerror(&error)));
        }
        // `agents .`'s folder was for this list only, not for what the agent starts
        let error = Command::new(&argv[0]).args(&argv[1..]).env_remove("AGENTS_HERE").exec();
        die(&format!("启动不了 {}：{}", argv[0], strerror(&error)));
    }

    fn launch(&self, what: &str, agent: &str, cwd: &str, extra: &[String], ident: &str) {
        let words: &[&str] = match (what, agent) {
            ("resume", "claude") => &["claude", "--resume", "{id}"],
            ("resume", "codex") => &["codex", "resume", "{id}"],
            ("resume", _) => &["pi", "--session", "{id}"],
            // a new conversation from this one's history; the original stays as it was. Pi forks into the
            // folder it runs in, so it, like the others, runs in the conversation's folder.
            ("fork", "claude") => &["claude", "--resume", "{id}", "--fork-session"],
            ("fork", "codex") => &["codex", "fork", "{id}"],
            ("fork", _) => &["pi", "--fork", "{id}"],
            _ => std::slice::from_ref(&agent),
        };
        let mut argv: Vec<String> = words.iter().map(|w| if *w == "{id}" { ident.to_string() } else { w.to_string() }).collect();
        argv.extend_from_slice(extra);
        self.run(agent, cwd, argv);
    }

    /// Where to start a new conversation: a folder used before, or a path typed in.
    fn choose_dir(&self, agent: &str) -> Option<String> {
        let mut seen = HashSet::new();
        let mut lines = Vec::new();
        let used = self.conversations("all").into_iter().map(|r| r.cwd);
        for d in getcwd().into_iter().chain(used).chain([self.home.clone()]) {
            if !seen.contains(&d) && plain_field(&d) && is_dir(&d) {
                lines.push(format!("{d}\t\t\t\t\t{}", self.short(&d)));
                seen.insert(d);
            }
        }
        let (code, out) = pick(
            &lines,
            &format!("在哪个目录新建 {} 对话 > ", name_of(agent)),
            "",
            "选一个用过的目录，或输入一个路径（/、~、./ 开头）后按 Enter",
            &["--print-query".to_string()],
        );
        // 0: a row was picked; 1: no row matched (the typed path may still be one); else Esc or an error
        if code != 0 && code != 1 {
            return None;
        }
        let mut out = out.split('\n');
        let query = out.next().unwrap_or("").trim_matches(is_space);
        let chosen = out.next().and_then(|row| row.split('\t').next()).unwrap_or("");
        // a typed path that exists wins over a row that merely matches it fuzzily; a path being typed
        // that is no folder (yet) is a filter: the row it picked is the answer
        if ["/", "~", "./", "../"].iter().any(|start| query.starts_with(start)) {
            let typed_dir = self.resolve(query);
            if is_dir(&typed_dir) {
                return Some(typed_dir);
            }
            if chosen.is_empty() {
                die(&format!("没有这个目录：{typed_dir}"));
            }
        }
        (!chosen.is_empty()).then(|| chosen.to_string())
    }

    /// A folder as written: `~` expanded, relative to here, normalized.
    fn resolve(&self, written: &str) -> String {
        let target = expanduser(written, self.home_env.as_deref());
        normpath(&if target.starts_with('/') { target } else { join(&here(), &target) })
    }

    fn new_conversation(&self, agent: &str, cwd: Option<String>, extra: &[String]) {
        if let Some(cwd) = cwd.or_else(|| self.choose_dir(agent)) {
            self.launch("new", agent, &cwd, extra, "");
        }
    }
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn read_from(path: &str, start: u64, length: u64) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut raw = Vec::with_capacity(length as usize);
    file.take(length).read_to_end(&mut raw).ok()?;
    Some(raw)
}

/// Waits for Enter, so a message stays up before fzf takes the screen back.
fn pause() {
    let _ = write!(io::stderr(), "按回车返回");
    let _ = io::stderr().flush();
    let _ = io::stdin().read_line(&mut String::new());
}

fn strerror(error: &io::Error) -> String {
    let text = error.to_string();
    // "Permission denied (os error 13)" -> "Permission denied"
    text.split(" (os error").next().unwrap_or(&text).to_string()
}

fn char_width(c: char) -> usize {
    let code = c as u32;
    let wide = wide::WIDE
        .binary_search_by(|(low, high)| {
            if code < *low {
                std::cmp::Ordering::Greater
            } else if code > *high {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok();
    if wide {
        2
    } else {
        1
    }
}

pub fn width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

pub fn pad(text: &str, cols: usize) -> String {
    let mut out = text.to_string();
    if width(text) > cols {
        let mut keep = Vec::new();
        let mut used = 0;
        for c in text.chars().rev() {
            if used + char_width(c) > cols.saturating_sub(1) {
                break;
            }
            used += char_width(c);
            keep.push(c);
        }
        out = std::iter::once('…').chain(keep.into_iter().rev()).collect();
    }
    let fill = cols.saturating_sub(width(&out));
    out + &" ".repeat(fill)
}

/// The first `count` characters.
fn chars_upto(text: &str, count: usize) -> &str {
    text.char_indices().nth(count).map_or(text, |(at, _)| &text[..at])
}

fn ago_from(now: f64, then: f64) -> String {
    let seconds = (now - then) as i64;
    if seconds < 3600 {
        format!("{}分钟前", seconds.div_euclid(60).max(1))
    } else if seconds < 86400 {
        format!("{}小时前", seconds / 3600)
    } else {
        format!("{}天前", seconds / 86400)
    }
}

fn tag(agent: &str) -> String {
    format!("{}{}{RESET}", color_of(agent), pad(&format!("[{}]", name_of(agent)), 8))
}

fn fzf_version() -> Option<(u32, u32)> {
    let mut fzf = Command::new(fzf_bin());
    fzf.arg("--version");
    let out = output_within(fzf, Duration::from_secs(5))?;
    let numbers = || -> Option<(u32, u32)> {
        let mut parts = out.split_whitespace().next()?.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        Some((major, minor))
    };
    Some(numbers().unwrap_or((0, 0)))
}

/// How fzf's bindings call this program back.
fn me() -> String {
    quote(&own_path())
}

fn pick(lines: &[String], prompt: &str, query: &str, header: &str, extra: &[String]) -> (i32, String) {
    let mut fzf = Command::new(fzf_bin());
    fzf.args(["--ansi", "--delimiter", "\t", "--with-nth", "6", "--no-sort", "--reverse", "--query", query, "--prompt", prompt, "--header", header]);
    fzf.args(extra).stdin(Stdio::piped()).stdout(Stdio::piped());
    let Ok(mut child) = fzf.spawn() else { die(NEED_FZF) };
    let mut stdin = child.stdin.take();
    let input = lines.join("\n");
    // fzf may leave before reading it all; what it did not read is no error
    let writer = thread::spawn(move || {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(input.as_bytes());
        }
    });
    let Ok(result) = child.wait_with_output() else { die(NEED_FZF) };
    let _ = writer.join();
    (result.status.code().unwrap_or(-1), String::from_utf8_lossy(&result.stdout).into_owned())
}

/// Where this program was started from, as the person sees it (a symlink stays a symlink).
fn invoked_as() -> String {
    let argv0 = std::env::args_os().next().map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
    if argv0.contains('/') {
        return abspath(&argv0);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':').map(|folder| join(folder, &argv0)).find(|candidate| executable(candidate) && !is_dir(candidate)).map_or_else(own_path, |found| abspath(&found))
}

/// Run the installer again, into the folder this copy lives in: the newest `agents`, and fzf if it is missing or old.
fn update(ctx: &Ctx) -> ! {
    let url = format!("https://raw.githubusercontent.com/{REPO}/main/install.sh");
    let have_curl = ctx.installed("curl");
    let fetch: Vec<&str> = if have_curl { vec!["curl", "-fsSL", &url] } else { vec!["wget", "-qO-", &url] };
    note(&format!("更新：{} | sh", fetch.join(" ")));
    let Ok(got) = Command::new(fetch[0]).args(&fetch[1..]).stdin(Stdio::inherit()).stderr(Stdio::inherit()).output() else {
        die("更新失败：需要 curl 或 wget");
    };
    if !got.status.success() || got.stdout.is_empty() {
        die(&format!("更新失败：下载 {url} 出错"));
    }
    let mut sh = Command::new("sh");
    sh.stdin(Stdio::piped());
    if std::env::var_os("AGENTS_BIN_DIR").is_none() {
        sh.env("AGENTS_BIN_DIR", dirname(&invoked_as()));
    }
    let Ok(mut child) = sh.spawn() else { die("更新失败：运行不了 sh") };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&got.stdout);
    }
    let code = child.wait().ok().and_then(|status| status.code()).unwrap_or(1);
    std::process::exit(code)
}

fn here() -> String {
    getcwd().unwrap_or_else(|| die("当前目录已经不在了：先 cd 到一个存在的目录"))
}

fn busy_message(agent: &str) -> String {
    let place = if agent == "claude" { "先在那边退出（claude agents 里的后台会话要先停掉）" } else { "先在那边退出" };
    format!("这个对话正被另一个 {} 进程占着：{place}；或者重新运行 agents，在它上面按 Ctrl-O 分出一份。", name_of(agent))
}

// ---------- the command ----------

pub fn main(argv: Vec<String>) {
    let mut ctx = Ctx::from_env();
    let (argv, extra): (&[String], &[String]) = match argv.iter().position(|a| a == "--") {
        Some(i) => (&argv[..i], &argv[i + 1..]),
        None => (&argv, &[]),
    };
    let rest = argv.get(1..).unwrap_or_default();
    match argv.first().map(String::as_str) {
        Some("--version") => return say(&format!("agents {VERSION}")),
        Some("-h" | "--help") => return say(HELP),
        Some("--list") => {
            let mode = rest.first().map(String::as_str).filter(|m| MODES.contains(m)).unwrap_or("all");
            let lines = ctx.rows_for(mode);
            if !lines.is_empty() {
                say(&lines.join("\n"));
            }
            return;
        }
        Some("--cycle") => {
            // --cycle [next|back] [footer|inline] [arrow]
            let has = |word: &str| rest.iter().any(|a| a == word);
            let query = std::env::var("FZF_QUERY").ok();
            return say(&ctx.cycle(if has("back") { -1 } else { 1 }, !has("inline"), has("arrow"), query.as_deref()));
        }
        Some("--preview") => {
            let field = |i: usize| rest.get(i).map_or("", String::as_str);
            return ctx.preview(field(0), field(1), field(2), field(3));
        }
        Some("--archive") => {
            // --archive <action> <agent> <id> <path> [footer|inline]
            let field = |i: usize| rest.get(i).map_or("", String::as_str);
            let actions = ctx.archive_key(field(0), field(1), field(2), field(3), field(4) != "inline");
            if !actions.is_empty() {
                say(&actions);
            }
            return;
        }
        Some("--delete") => {
            // --delete <action> <agent> <id> <cwd> <path>
            let field = |i: usize| rest.get(i).map_or("", String::as_str);
            return ctx.delete_key(field(0), field(1), field(2), field(3), field(4));
        }
        Some("--refresh") => {
            let mode = ctx.remembered();
            ctx.remember(&mode);
            return say(&ctx.show(&mode, !rest.iter().any(|a| a == "inline")));
        }
        Some("update") => update(&ctx),
        Some("new") => return command_new(&ctx, rest, extra),
        _ => {}
    }

    let mut mode = None;
    let mut words = Vec::new();
    for a in argv {
        if MODES.iter().any(|m| a.strip_prefix("--") == Some(*m)) {
            mode = Some(a[2..].to_string());
        } else if a == "." || a == "--here" {
            let folder = here();
            // fzf's reloads find it in the environment
            std::env::set_var("AGENTS_HERE", &folder);
            ctx.here = Some(folder);
        } else {
            words.push(a.as_str());
        }
    }
    let mode = mode.unwrap_or_else(|| ctx.remembered());
    ctx.remember(&mode);
    let Some(version) = fzf_version() else { die(NEED_FZF) };
    let own = me();
    let mut keys: Vec<String> = ["--expect", "ctrl-n,ctrl-o", "--preview"].map(String::from).to_vec();
    keys.push(format!("{own} --preview {{2}} {{5}} {{3}} {{4}}"));
    keys.extend(["--preview-window", "down,45%,wrap", "--bind", "ctrl-/:toggle-preview"].map(String::from));
    let footer = version >= FZF_FOOTER;
    let layout = if footer { "footer" } else { "inline" };
    let top = if version >= FZF_SWITCH {
        let forward = format!("transform:{own} --cycle next {layout}");
        let back = format!("transform:{own} --cycle back {layout}");
        // ←/→ switch lists while nothing is typed, and move in the search text once something is.
        // The check runs in sh, so moving in the text starts no process of ours (fzf 0.51+: --with-shell).
        let (right, left) = if version >= FZF_WITH_SHELL {
            keys.extend(["--with-shell", "sh -c"].map(String::from));
            (
                format!("transform:[ -n \"$FZF_QUERY\" ] && echo forward-char || {own} --cycle next {layout}"),
                format!("transform:[ -n \"$FZF_QUERY\" ] && echo backward-char || {own} --cycle back {layout}"),
            )
        } else {
            (format!("{forward} arrow"), format!("{back} arrow"))
        };
        // Ctrl-X works without leaving the list; Ctrl-D asks on the terminal first
        let archive = format!("ctrl-x:transform:{own} --archive {{1}} {{2}} {{3}} {{5}} {layout}");
        let delete = format!("ctrl-d:execute({own} --delete {{1}} {{2}} {{3}} {{4}} {{5}})+transform:{own} --refresh {layout}");
        for bind in [format!("right:{right}"), format!("left:{left}"), format!("tab:{forward}"), format!("btab:{back}"), format!("ctrl-t:{forward}"), archive, delete] {
            keys.push("--bind".to_string());
            keys.push(bind);
        }
        ctx.header(&mode, footer)
    } else {
        format!("{DIM}{ACTION_HELP}{TAIL_HELP}{RESET}  （fzf 0.46 以上才能切换列表、归档和删除：运行 agents update）")
    };
    if footer {
        keys.push("--footer".to_string());
        keys.push(format!("{DIM}{SWITCH_HELP}{ACTION_HELP}{TIDY_HELP}{TAIL_HELP}{RESET}"));
    }
    let (_, out) = pick(&ctx.rows_for(&mode), "> ", &words.join(" "), &top, &keys);
    let mut out = out.split('\n');
    let key = out.next().unwrap_or("");
    let Some(row) = out.next().filter(|row| !row.is_empty()) else { return };
    let fields: Vec<&str> = row.split('\t').collect();
    if fields.len() < 5 {
        return;
    }
    let Some(agent) = agent_named(fields[1]) else { return };
    let (action, ident, cwd, path) = (fields[0], fields[2], fields[3], fields[4]);
    // going on with an archived conversation (or a fork of it) takes it out of the archive first
    if action == "archived" && key != "ctrl-n" {
        if let Err(e) = ctx.archive(agent, ident, path, false) {
            die(&format!("取回失败：{e}"));
        }
    }
    if action == "new" {
        return ctx.new_conversation(agent, None, extra);
    }
    if key == "ctrl-n" {
        return ctx.new_conversation(agent, Some(cwd.to_string()), extra);
    }
    if key == "ctrl-o" {
        return ctx.launch("fork", agent, cwd, extra, ident);
    }
    if ctx.held(&ctx.conversations(agent)).contains(ident) {
        die(&busy_message(agent));
    }
    ctx.launch("resume", agent, cwd, extra, ident);
}

fn command_new(ctx: &Ctx, rest: &[String], extra: &[String]) {
    let (agent, rest): (String, &[String]) = match rest.first() {
        Some(first) if AGENTS.contains(&first.as_str()) => (first.clone(), &rest[1..]),
        _ => {
            let last = ctx.remembered();
            let agent = if AGENTS.contains(&last.as_str()) && ctx.installed(&last) {
                last
            } else {
                AGENTS.iter().find(|a| ctx.installed(a)).copied().unwrap_or("claude").to_string()
            };
            (agent, rest)
        }
    };
    if rest.len() > 1 {
        die("用法：agents new [claude|codex|pi] [目录] [-- 给 agent 的参数]");
    }
    let cwd = rest.first().map(|written| {
        let cwd = ctx.resolve(written);
        if !is_dir(&cwd) {
            let hint = if written.contains('/') { "" } else { "（agent 可以是 claude、codex、pi）" };
            die(&format!("没有这个目录：{cwd}{hint}"));
        }
        cwd
    });
    ctx.new_conversation(&agent, cwd, extra);
}
