//! agents against made-up Claude Code, Codex and Pi homes. Run: cargo test
use agents::{alive, held_by_command_line, lock_holders, normpath, pad, parse, realpath, typed, width, Conversation, Ctx};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

const AGENTS_BIN: &str = env!("CARGO_BIN_EXE_agents");

/// Stands in for fzf: picks the first row holding $FAKE_FZF_PICK, presses $FAKE_FZF_KEY, types $FAKE_FZF_QUERY.
/// Each call's arguments land on one line of $FAKE_FZF_LOG, separated by \036.
const FAKE_FZF: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "${FAKE_FZF_VERSION:-0.74.5} (fake)"; exit 0; fi
pq=0; ex=0
for a in "$@"; do
  printf '%s\036' "$a" >> "$FAKE_FZF_LOG"
  case "$a" in --print-query) pq=1 ;; --expect) ex=1 ;; esac
done
printf '\n' >> "$FAKE_FZF_LOG"
chosen="$(awk 'ENVIRON["FAKE_FZF_PICK"] != "" && index($0, ENVIRON["FAKE_FZF_PICK"]) { print; exit }')"
[ "$pq" = 1 ] && printf '%s\n' "$FAKE_FZF_QUERY"
[ "$ex" = 1 ] && printf '%s\n' "$FAKE_FZF_KEY"
if [ -n "$chosen" ]; then printf '%s\n' "$chosen"; exit 0; fi
exit 1
"#;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// `json.dumps`' default look: a space after every colon and comma.
struct Spaced;

impl serde_json::ser::Formatter for Spaced {
    fn begin_object_value<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
    fn begin_object_key<W: ?Sized + std::io::Write>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }
    fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }
}

fn dumps(value: &Value, compact: bool) -> String {
    if compact {
        return value.to_string();
    }
    let mut out = Vec::new();
    let mut writer = serde_json::Serializer::with_formatter(&mut out, Spaced);
    serde::Serialize::serialize(value, &mut writer).unwrap();
    String::from_utf8(out).unwrap()
}

fn user(cwd: &str, content: Value) -> Value {
    json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": content}})
}

fn user_with(cwd: &str, content: Value, key: &str, value: Value) -> Value {
    let mut row = user(cwd, content);
    row[key] = value;
    row
}

fn codex_user(content: Value) -> Value {
    let content = if content.is_array() { content } else { json!([{"type": "input_text", "text": content}]) };
    json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": content}})
}

fn write_lines(path: &str, lines: &[String]) {
    fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
    fs::write(path, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
}

struct Home {
    tmp: String,
    claude: String,
    codex: String,
    pi: String,
    work: String,
    game: String,
    sub: String,
    state: String,
    projects: String,
    codex_day: String,
    pi_game: String,
    bin: String,
    log: String,
    files: HashMap<String, String>,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.tmp);
    }
}

impl Home {
    fn new() -> Home {
        let unique = format!("agents-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst));
        let tmp = std::env::temp_dir().join(unique);
        fs::create_dir_all(&tmp).unwrap();
        // macOS: /var/folders is /private/var/folders
        let tmp = fs::canonicalize(&tmp).unwrap().to_string_lossy().into_owned();
        let at = |name: &str| format!("{tmp}/{name}");
        let (claude, codex, pi) = (at("claude"), at("codex"), at("pi"));
        let (work, game, sub) = (at("work"), at("game"), at("game/sub"));
        for d in [&work, &sub, &at("game2"), &at("bin")] {
            fs::create_dir_all(d).unwrap();
        }
        let mut h = Home {
            state: at("state"),
            bin: at("bin"),
            log: at("fzf.log"),
            projects: format!("{claude}/projects"),
            codex_day: format!("{codex}/sessions/2026/11/03"),
            pi_game: format!("{pi}/sessions/--game--"),
            files: HashMap::new(),
            claude, codex, pi, work, game, sub, tmp,
        };
        let (w, g, s) = (h.work.clone(), h.game.clone(), h.sub.clone());
        h.add("aaaa", &format!("{}/-work/aaaa.jsonl", h.projects), &[
            json!({"type": "permission-mode", "permissionMode": "default"}),
            user_with(&w, json!("<local-command-caveat>x</local-command-caveat>"), "isMeta", json!(true)),
            user(&w, json!("fix the login bug")),
            json!({"type": "assistant", "cwd": w, "message": {"role": "assistant", "content": [{"type": "text", "text": "Fixed it."}]}}),
            json!({"type": "ai-title", "aiTitle": "Login bug"}),
            json!({"type": "custom-title", "customTitle": "登录问题"}),
        ], false);
        h.add("bbbb", &format!("{}/-game/bbbb.jsonl", h.projects), &[
            user(&g, json!([{"type": "text", "text": "make a pinball game"}])),
            json!({"type": "ai-title", "aiTitle": "Pinball"}),
        ], true);
        h.add("cccc", &format!("{}/-game/cccc.jsonl", h.projects), &[json!({"type": "mode", "mode": "normal"})], false);
        h.add("sub1", &format!("{}/-game/bbbb/subagents/sub1.jsonl", h.projects), &[user(&g, json!("subagent work"))], false);
        h.add("dddd4444", &format!("{}/rollout-2026-11-03T10-00-00-dddd4444.jsonl", h.codex_day), &[
            json!({"type": "session_meta", "payload": {"id": "dddd4444", "cwd": w, "source": "cli"}}),
            codex_user(json!("<environment_context>cwd</environment_context>")),
            codex_user(json!("write the release notes")),
            json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done."}]}}),
        ], false);
        h.add("eeee5555", &format!("{}/rollout-2026-11-03T11-00-00-eeee5555.jsonl", h.codex_day), &[
            json!({"type": "session_meta", "payload": {"id": "eeee5555", "cwd": s, "source": "vscode"}}),
            codex_user(json!("add a score board")),
        ], false);
        h.add("ffff6666", &format!("{}/rollout-2026-11-03T12-00-00-ffff6666.jsonl", h.codex_day), &[
            json!({"type": "session_meta", "payload": {"id": "ffff6666", "cwd": w, "source": {"subagent": {"other": "guardian"}}}}),
            codex_user(json!("review this command")),
        ], false);
        write_lines(&format!("{}/session_index.jsonl", h.codex), &[
            dumps(&json!({"id": "dddd4444", "thread_name": "old name"}), false),
            dumps(&json!({"id": "dddd4444", "thread_name": "发布说明"}), false),
        ]);
        h.add("hhhh8888", &format!("{}/2026-11-01T09-00-00-000Z_hhhh8888.jsonl", h.pi_game), &[
            json!({"type": "session", "version": 3, "id": "hhhh8888", "cwd": g}),
            json!({"type": "message", "id": "a1", "parentId": null, "message": {"role": "user", "content": "tune the flippers"}}),
            json!({"type": "message", "id": "a2", "parentId": "a1", "message": {"role": "assistant", "content": [{"type": "text", "text": "Tuned."}]}}),
            json!({"type": "session_info", "id": "a3", "parentId": "a2", "name": "弹珠手感"}),
        ], false);
        h.add("iiii9999", &format!("{}/2026-11-01T08-00-00-000Z_iiii9999.jsonl", h.pi_game), &[
            json!({"type": "session", "version": 3, "id": "iiii9999", "cwd": g}),
        ], false);
        // newest first: eeee5555, bbbb, dddd4444, aaaa, hhhh8888
        for (i, name) in ["hhhh8888", "aaaa", "dddd4444", "bbbb", "eeee5555"].iter().enumerate() {
            let when = UNIX_EPOCH + Duration::from_secs(1_800_000_000 + i as u64 * 60);
            fs::File::options().write(true).open(&h.files[*name]).unwrap().set_modified(when).unwrap();
        }
        for (name, body) in [("claude", "#!/bin/sh\n"), ("codex", "#!/bin/sh\n"), ("pi", "#!/bin/sh\n"), ("fzf", FAKE_FZF)] {
            h.script(name, body);
        }
        h
    }

    fn script(&self, name: &str, body: &str) {
        let path = format!("{}/{name}", self.bin);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn add(&mut self, ident: &str, path: &str, rows: &[Value], compact: bool) {
        write_lines(path, &rows.iter().map(|r| dumps(r, compact)).collect::<Vec<_>>());
        self.files.insert(ident.to_string(), path.to_string());
    }

    fn env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("CLAUDE_CONFIG_DIR".to_string(), self.claude.clone()),
            ("CODEX_HOME".to_string(), self.codex.clone()),
            ("PI_CODING_AGENT_DIR".to_string(), self.pi.clone()),
            ("XDG_STATE_HOME".to_string(), self.state.clone()),
        ];
        env.push(("PATH".to_string(), format!("{}:/usr/bin:/bin", self.bin)));
        env
    }

    /// The program's view of these homes, with `path` as its $PATH.
    fn ctx_on(&self, path: &str, extra: &[(&str, &str)]) -> Ctx {
        let env = self.env();
        Ctx::with(|name| {
            if name == "PATH" {
                return Some(path.to_string());
            }
            if let Some((_, value)) = extra.iter().find(|(key, _)| *key == name) {
                return Some(value.to_string());
            }
            match name {
                "HOME" => std::env::var("HOME").ok(),
                _ => env.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()),
            }
        })
    }

    fn ctx(&self) -> Ctx {
        self.ctx_on(&format!("{}:/usr/bin:/bin", self.bin), &[])
    }

    fn ids(&self, mode: &str) -> Vec<String> {
        self.ctx().conversations(mode).into_iter().map(|r| r.id).collect()
    }

    fn titles(&self) -> HashMap<String, String> {
        self.ctx().conversations("all").into_iter().map(|r| (r.id, r.title)).collect()
    }

    fn shown(&self, mode: &str) -> HashMap<String, String> {
        self.ctx()
            .rows_for(mode)
            .iter()
            .filter(|r| r.starts_with("resume"))
            .map(|r| {
                let fields: Vec<&str> = r.split('\t').collect();
                (fields[2].to_string(), fields[5].to_string())
            })
            .collect()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(AGENTS_BIN);
        command.args(args).envs(self.env()).env_remove("PI_CODING_AGENT_SESSION_DIR").env_remove("AGENTS_HERE").env_remove("AGENTS_DRY_RUN").current_dir(&self.work);
        command
    }

    fn run_agents(&self, args: &[&str], pick: &str, key: &str, query: &str) -> (Output, Option<Value>) {
        let out = self
            .command(args)
            .env("AGENTS_DRY_RUN", "1")
            .env("FAKE_FZF_LOG", &self.log)
            .env("FAKE_FZF_PICK", pick)
            .env("FAKE_FZF_KEY", key)
            .env("FAKE_FZF_QUERY", query)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let launched = if text.starts_with('{') { Some(serde_json::from_str(&text).unwrap()) } else { None };
        (out, launched)
    }

    fn fzf_calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(&self.log)
            .unwrap()
            .lines()
            .map(|line| line.split('\x1e').filter(|a| !a.is_empty()).map(str::to_string).collect())
            .collect()
    }

    fn preview(&self, agent: &str, ident: &str) -> String {
        let out = self.command(&["--preview", agent, &self.files[ident], ident, &self.work]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn after<'a>(args: &'a [String], flag: &str) -> &'a str {
    &args[args.iter().position(|a| a == flag).unwrap() + 1]
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn set(items: &[&str]) -> HashSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ---------- reading ----------

#[test]
fn lists_every_agent_newest_first() {
    assert_eq!(Home::new().ids("all"), ["eeee5555", "bbbb", "dddd4444", "aaaa", "hhhh8888"]);
}

#[test]
fn titles() {
    let t = Home::new().titles();
    assert_eq!(t["aaaa"], "登录问题"); // /rename beats the AI title
    assert_eq!(t["bbbb"], "Pinball");
    assert_eq!(t["dddd4444"], "发布说明"); // the last name in Codex's index
    assert_eq!(t["eeee5555"], "add a score board"); // unnamed: the first thing typed
    assert_eq!(t["hhhh8888"], "弹珠手感"); // Pi's /name
}

#[test]
fn one_agent() {
    let h = Home::new();
    assert_eq!(h.ids("codex"), ["eeee5555", "dddd4444"]);
    assert_eq!(h.ids("claude"), ["bbbb", "aaaa"]);
    assert_eq!(h.ids("pi"), ["hhhh8888"]);
}

#[test]
fn skips_empty_conversations_and_helpers() {
    let ids = Home::new().ids("all");
    for left_out in ["cccc", "sub1", "ffff6666", "iiii9999"] {
        assert!(!ids.iter().any(|id| id == left_out), "{left_out}");
    }
}

#[test]
fn cwd_comes_from_the_transcript() {
    let h = Home::new();
    let cwd: HashMap<String, String> = h.ctx().conversations("all").into_iter().map(|r| (r.id, r.cwd)).collect();
    assert_eq!(cwd["aaaa"], h.work);
    assert_eq!(cwd["eeee5555"], h.sub);
}

#[test]
fn a_prompt_after_injected_blocks() {
    // Codex stores an attached image as marker blocks around it; Claude puts an IDE selection first
    let mut h = Home::new();
    let (w, day, projects) = (h.work.clone(), h.codex_day.clone(), h.projects.clone());
    h.add("img1", &format!("{day}/rollout-2026-11-03T13-00-00-img1.jsonl"), &[
        json!({"type": "session_meta", "payload": {"id": "img1", "cwd": w, "source": "cli"}}),
        codex_user(json!([{"type": "input_text", "text": "<image name=[Image #1]>"}, {"type": "input_image", "image_url": "data:"},
                          {"type": "input_text", "text": "</image>"}, {"type": "input_text", "text": "[Image #1] what can you see"}])),
    ], false);
    h.add("ide1", &format!("{projects}/-work/ide1.jsonl"), &[
        user(&w, json!([{"type": "text", "text": "<ide_selection>x = 1</ide_selection>"}, {"type": "text", "text": "typed question"}])),
    ], false);
    let t = h.titles();
    assert_eq!(t["img1"], "[Image #1] what can you see");
    assert_eq!(t["ide1"], "typed question");
}

#[test]
fn a_prompt_that_starts_with_a_tag_is_still_a_prompt() {
    let mut h = Home::new();
    let (w, projects) = (h.work.clone(), h.projects.clone());
    h.add("tag1", &format!("{projects}/-work/tag1.jsonl"), &[user(&w, json!("<div> why is this broken"))], false);
    assert_eq!(h.titles()["tag1"], "<div> why is this broken");
}

#[test]
fn a_conversation_started_by_a_command() {
    let mut h = Home::new();
    let (w, projects) = (h.work.clone(), h.projects.clone());
    let command = "<command-name>/review</command-name>\n<command-message>review</command-message>\n<command-args>PR 7</command-args>";
    h.add("cmd1", &format!("{projects}/-work/cmd1.jsonl"), &[
        user(&w, json!(command)),
        json!({"type": "assistant", "cwd": w, "message": {"model": "claude-opus-5-5", "role": "assistant", "content": [{"type": "text", "text": "Reviewing."}]}}),
    ], false);
    // a command that changed a setting and got no answer is no conversation to resume
    h.add("cmd2", &format!("{projects}/-work/cmd2.jsonl"), &[
        user(&w, json!("<command-name>/model</command-name>")),
        json!({"type": "assistant", "cwd": w, "message": {"model": "<synthetic>", "role": "assistant", "content": [{"type": "text", "text": "No response requested."}]}}),
    ], false);
    let t = h.titles();
    assert_eq!(t["cmd1"], "/review PR 7");
    assert!(!t.contains_key("cmd2"));
}

#[test]
fn typed_text_after_many_tool_results() {
    let mut h = Home::new();
    let (w, projects) = (h.work.clone(), h.projects.clone());
    let mut rows = vec![user(&w, json!("<command-name>/init</command-name>"))];
    rows.extend((0..400).map(|i| user(&w, json!([{"type": "tool_result", "tool_use_id": i.to_string(), "content": "ok"}]))));
    rows.push(user(&w, json!("and now the tests")));
    h.add("late1", &format!("{projects}/-work/late1.jsonl"), &rows, true);
    assert_eq!(h.titles()["late1"], "and now the tests");
}

#[test]
fn broken_files_are_skipped_not_fatal() {
    let h = Home::new();
    let bad = format!("{}/-bad", h.projects);
    // a lone surrogate escape, as JavaScript writes a string cut mid-emoji
    let surrogate = format!(r#"{{"type": "user", "cwd": "{}", "message": {{"role": "user", "content": "broken \ud83d emoji"}}}}"#, h.work);
    write_lines(&format!("{bad}/s1.jsonl"), &[surrogate]);
    write_lines(&format!("{bad}/s2.jsonl"), &[dumps(&json!({"type": "user", "cwd": h.work, "message": "just a string"}), false)]);
    write_lines(&format!("{bad}/s3.jsonl"), &[format!(r#"{{"type":"user","cwd":"{}","message":{}"#, h.work, "[".repeat(100_000))]);
    write_lines(&format!("{bad}/s4.jsonl"), &[dumps(&user_with("x", json!("cwd is a number"), "cwd", json!(7)), false)]);
    fs::write(format!("{bad}/s5.jsonl"), b"\x00\xff garbage\n\x89PNG").unwrap();
    write_lines(&format!("{}/rollout-x-bad1.jsonl", h.codex_day), &[dumps(&json!({"type": "session_meta", "payload": {"id": ["x"], "cwd": 3}}), false)]);
    write_lines(&format!("{}/rollout-x-bad2.jsonl", h.codex_day), &["[1, 2, 3]".to_string()]);
    write_lines(&format!("{}/bad.jsonl", h.pi_game), &[dumps(&json!({"type": "session", "id": {"x": 1}, "cwd": h.game}), false)]);
    let mut index = fs::File::options().append(true).open(format!("{}/session_index.jsonl", h.codex)).unwrap();
    writeln!(index, "{}", dumps(&json!({"id": ["x"], "thread_name": "y"}), false)).unwrap();
    fs::create_dir_all(format!("{}/sessions", h.claude)).unwrap();
    fs::write(format!("{}/sessions/9.json", h.claude), dumps(&json!({"pid": std::process::id(), "sessionId": ["x"]}), false)).unwrap();
    let ids = h.ids("all");
    assert!(ids.iter().any(|id| id == "aaaa"));
    assert!(ids.iter().any(|id| id == "s1")); // a lone surrogate is still a conversation
    let out = h.command(&["--list", "all"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("broken ? emoji"));
}

#[test]
fn more_titles_and_prompts() {
    let mut h = Home::new();
    let (w, projects) = (h.work.clone(), h.projects.clone());
    h.add("named1", &format!("{projects}/-work/named1.jsonl"), &[user(&w, json!("hello")), json!({"type": "agent-name", "agentName": "后台任务"})], false);
    h.add("tag2", &format!("{projects}/-work/tag2.jsonl"), &[user(&w, json!("<image> tag has no alt"))], false);
    h.add("compact1", &format!("{projects}/-work/compact1.jsonl"), &[
        user_with(&w, json!("This session is being continued from a previous conversation"), "isCompactSummary", json!(true)),
    ], false);
    write_lines(&format!("{projects}/-work/cwd2.jsonl"), &[r#"{"type":"x","cwd":"/tmp", TRUNCATED"#.to_string(), dumps(&user(&w, json!("after a broken line")), true)]);
    h.add("cr1", &format!("{projects}/-work/cr1.jsonl"), &[user(&format!("{w}\rb"), json!("carriage return folder"))], false);
    let t = h.titles();
    assert_eq!(t["named1"], "后台任务");
    assert_eq!(t["tag2"], "<image> tag has no alt");
    assert!(!t.contains_key("compact1")); // a compaction summary is nothing the person said
    assert_eq!(t["cwd2"], "after a broken line");
    assert!(!t.contains_key("cr1"));
}

#[test]
fn pi_session_folder_settings() {
    let h = Home::new();
    let elsewhere = format!("{}/pi-elsewhere", h.tmp);
    fs::rename(format!("{}/sessions", h.pi), &elsewhere).unwrap();
    fs::write(format!("{}/settings.json", h.pi), dumps(&json!({"sessionDir": elsewhere}), false)).unwrap();
    assert_eq!(h.ctx().pi_sessions, elsewhere);
    assert_eq!(h.ids("pi"), ["hhhh8888"]);
    let other = format!("{}/pi-env", h.tmp);
    assert_eq!(h.ctx_on("", &[("PI_CODING_AGENT_SESSION_DIR", &other)]).pi_sessions, other);
    // a relative sessionDir means something to Pi alone: <dir>/sessions it stays
    fs::write(format!("{}/settings.json", h.pi), r#"{"sessionDir": "relative/folder"}"#).unwrap();
    assert_eq!(h.ctx().pi_sessions, format!("{}/sessions", h.pi));
}

#[test]
fn this_folder_and_below_only() {
    let h = Home::new();
    let ctx = h.ctx_on("", &[("AGENTS_HERE", &h.game)]);
    let cwds: HashSet<String> = ctx.conversations("all").into_iter().map(|r| r.cwd).collect();
    assert_eq!(cwds, HashSet::from([h.game.clone(), h.sub.clone()])); // not the sibling game2, not work
}

// ---------- what a running agent holds ----------

#[test]
fn claude_by_session_record() {
    let h = Home::new();
    let sessions = format!("{}/sessions", h.claude);
    fs::create_dir_all(&sessions).unwrap();
    let pid = std::process::id();
    let stat = fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let started = stat.rfind(')').and_then(|i| stat[i + 2..].split_whitespace().nth(19)).map(str::to_string);
    let mut records = vec![
        json!({"pid": pid, "sessionId": "aaaa", "procStart": started}),
        json!({"pid": 999_999_999, "sessionId": "bbbb"}),
    ];
    if started.is_some() {
        // the same pid, but a process started at another time: the record is stale
        records.push(json!({"pid": pid, "sessionId": "zzzz", "procStart": "1"}));
    }
    for (i, record) in records.iter().enumerate() {
        fs::write(format!("{sessions}/{i}.json"), dumps(record, false)).unwrap();
    }
    assert_eq!(h.ctx().held_claude(), set(&["aaaa"]));
}

#[test]
fn lock_holders_reads_proc_locks() {
    let h = Home::new();
    let locks = format!("{}/locks", h.tmp);
    fs::write(
        &locks,
        "48: FLOCK  ADVISORY  WRITE 1009782 08:01:786440 0 EOF\n48: -> FLOCK  ADVISORY  WRITE 1009999 08:01:786440 0 EOF\n49: POSIX  ADVISORY  READ 77 00:2a:12 0 EOF\n",
    )
    .unwrap();
    assert_eq!(lock_holders(&locks), HashMap::from([(786440, HashSet::from([1009782])), (12, HashSet::from([77]))]));
}

#[test]
fn codex_by_writer_lock_held_by_codex() {
    let h = Home::new();
    let locks = format!("{}/thread-writer-locks", h.codex);
    fs::create_dir_all(&locks).unwrap();
    for name in ["dddd4444.lock", "eeee5555.lock", ".coordination.lock"] {
        fs::write(format!("{locks}/{name}"), "").unwrap();
    }
    let inode = |name: &str| fs::metadata(format!("{locks}/{name}")).unwrap().ino();
    let ctx = h.ctx();
    let rows = ctx.conversations("codex");
    let argv = |pid: i32| -> Vec<String> {
        match pid {
            100 => vec!["/usr/bin/codex".to_string()],
            200 => vec!["vim".to_string(), "notes.txt".to_string()],
            _ => Vec::new(),
        }
    };
    // codex holds dddd4444's lock; a vim holding an inode that happens to match eeee5555's is no codex
    let holders = HashMap::from([(inode("dddd4444.lock"), HashSet::from([100])), (inode("eeee5555.lock"), HashSet::from([200]))]);
    assert_eq!(ctx.held_codex(&rows, &[], &holders, &argv), set(&["dddd4444"]));
}

#[test]
fn codex_and_pi_by_command_line() {
    let h = Home::new();
    let rows: Vec<Conversation> = h.ctx().conversations("all");
    let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    let procs = vec![
        argv(&["codex", "-c", "x=1", "resume", "eeee5555"]),
        argv(&["codex", "fork", "dddd4444"]), // a fork holds its new conversation, not the source
        argv(&["vim", "resume", "dddd4444"]),
        argv(&["node", "/home/u/.local/bin/pi", "--session", "hhhh8888"]),
    ];
    assert_eq!(held_by_command_line(&rows, "codex", &procs), set(&["eeee5555"]));
    assert_eq!(held_by_command_line(&rows, "pi", &procs), set(&["hhhh8888"]));
    assert_eq!(held_by_command_line(&rows, "pi", &[argv(&["node", "/x/pi-coding-agent/dist/cli.js", "--fork", "hhhh8888"])]), set(&[]));
    assert_eq!(held_by_command_line(&rows, "pi", &[argv(&["pi", "--session=hhhh8888"])]), set(&["hhhh8888"]));
    assert_eq!(held_by_command_line(&rows, "pi", &[argv(&["pi", "--session-id", "hhhh8888"])]), set(&["hhhh8888"]));
}

#[test]
fn alive_tells_gone_from_running() {
    assert!(alive(i64::from(std::process::id()), None));
    assert!(!alive(999_999_999, None));
    assert!(!alive(1 << 40, None));
}

// ---------- the lists ----------

#[test]
fn tab_goes_through_every_present_agent_and_shift_tab_back() {
    let h = Home::new();
    let ctx = h.ctx();
    for nxt in ["claude", "codex", "pi", "all", "claude"] {
        assert!(ctx.cycle(1, true, false, None).contains(&format!("--list {nxt}")));
        assert_eq!(ctx.remembered(), nxt);
    }
    assert!(ctx.cycle(-1, true, false, None).contains("--list all"));
    assert!(ctx.cycle(-1, true, false, None).contains("--list pi"));
}

#[test]
fn the_switcher_highlights_the_list_shown() {
    let h = Home::new();
    let ctx = h.ctx();
    let out = ctx.cycle(1, true, false, None); // all -> claude
    assert!(out.rsplit('+').next().unwrap().starts_with("change-header:")); // takes the rest of the line: last
    let bar = ctx.switcher("codex");
    for label in ["全部", "Claude", "Codex", "Pi"] {
        assert!(bar.contains(&format!(" {label} ")));
    }
    assert!(bar.contains("\x1b[1;7m Codex ")); // reverse video on the list shown
    assert!(!bar.contains("\x1b[1;7m Claude "));
    assert!(ctx.header("all", false).contains("切换列表")); // no footer: key help rides along
    assert!(!ctx.header("all", true).contains("切换列表"));
}

#[test]
fn an_arrow_moves_in_typed_text() {
    let h = Home::new();
    let ctx = h.ctx();
    assert_eq!(ctx.cycle(1, true, true, Some("login")), "forward-char");
    assert_eq!(ctx.cycle(-1, true, true, Some("login")), "backward-char");
    assert!(ctx.cycle(1, true, false, Some("login")).contains("--list claude")); // Tab switches whatever is typed
    assert!(ctx.cycle(1, true, true, Some("")).contains("--list codex"));
}

#[test]
fn the_cycle_command_reads_the_query_from_fzf() {
    let h = Home::new();
    let out = h.command(&["--cycle", "next", "footer", "arrow"]).env("FZF_QUERY", "x").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "forward-char");
    let out = h.command(&["--cycle", "back", "inline"]).env("FZF_QUERY", "x").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("--list pi") && text.contains("切换列表"), "{text}");
}

#[test]
fn cycle_skips_an_agent_with_nothing_here() {
    let h = Home::new();
    fs::remove_dir_all(&h.pi).unwrap();
    let ctx = h.ctx_on("", &[]); // nothing installed
    ctx.remember("codex");
    assert!(ctx.cycle(1, true, false, None).contains("--list all"));
    assert!(!ctx.switcher("all").contains(" Pi "));
    // a folder with no conversation files does not count either
    fs::create_dir_all(format!("{}/sessions", h.pi)).unwrap();
    assert!(!ctx.lists().contains(&"pi"));
}

#[test]
fn remembers_the_last_list() {
    let h = Home::new();
    let ctx = h.ctx();
    assert_eq!(ctx.remembered(), "all");
    ctx.cycle(1, true, false, None);
    ctx.cycle(1, true, false, None);
    assert_eq!(h.ctx().remembered(), "codex");
}

#[test]
fn the_mixed_list_names_each_agent() {
    let h = Home::new();
    let shown = h.shown("all");
    assert!(shown["aaaa"].contains("[Claude]"));
    assert!(shown["dddd4444"].contains("[Codex]"));
    assert!(shown["hhhh8888"].contains("[Pi]"));
    assert!(!h.shown("codex")["dddd4444"].contains("[Codex]"));
}

#[test]
fn rows_carry_tab_separated_fields() {
    let h = Home::new();
    let rows: Vec<Vec<String>> = h.ctx().rows_for("all").iter().filter(|r| r.starts_with("resume")).map(|r| r.split('\t').map(str::to_string).collect()).collect();
    assert!(rows.iter().all(|r| r.len() == 6));
    assert_eq!(rows[0][1..4], ["codex", "eeee5555", h.sub.as_str()]);
}

#[test]
fn the_first_prompt_is_searchable() {
    let mut h = Home::new();
    assert!(h.shown("all")["aaaa"].contains("fix the login bug"));
    let long = "x".repeat(200);
    let (w, projects) = (h.work.clone(), h.projects.clone());
    h.add("long1", &format!("{projects}/-work/long1.jsonl"), &[user(&w, json!(long))], false);
    assert!(h.shown("claude")["long1"].contains(&long)); // the title shows 140, the rest stays searchable
}

#[test]
fn new_rows_for_installed_agents() {
    let h = Home::new();
    let only = format!("{}/only", h.tmp);
    fs::create_dir_all(&only).unwrap();
    let codex = format!("{only}/codex");
    fs::write(&codex, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
    let rows = h.ctx_on(&only, &[]).rows_for("all");
    let news: Vec<&str> = rows.iter().filter(|r| r.starts_with("new")).map(|r| r.split('\t').nth(1).unwrap()).collect();
    assert_eq!(news, ["codex"]);
}

#[test]
fn pad_counts_wide_characters() {
    assert_eq!(width(&pad("登录", 6)), 6);
    assert_eq!(width(&pad("~/a/very/long/folder/name", 10)), 10);
    assert!(!pad("59分钟前", 8).contains('…'));
    assert_eq!(pad("~/a/very/long/folder/name", 10), "…lder/name");
}

#[test]
fn preview_of_each_agent() {
    let h = Home::new();
    let out = h.preview("codex", "dddd4444");
    assert!(out.contains("write the release notes"));
    assert!(out.contains("Done."));
    assert!(!out.contains("environment_context"));
    assert!(h.preview("claude", "bbbb").contains("make a pinball game")); // a one-line file keeps its first line
    assert!(h.preview("pi", "hhhh8888").contains("Tuned."));
}

#[test]
fn preview_looks_further_back() {
    let mut h = Home::new();
    let (w, projects) = (h.work.clone(), h.projects.clone());
    let mut rows = vec![user(&w, json!("the early question"))];
    rows.extend((0..900).map(|i| user(&w, json!([{"type": "tool_result", "tool_use_id": i.to_string(), "content": "y".repeat(1000)}]))));
    h.add("deep1", &format!("{projects}/-work/deep1.jsonl"), &rows, true);
    assert!(h.preview("claude", "deep1").contains("the early question"));
}

// ---------- the command ----------

#[test]
fn enter_resumes_in_the_conversations_folder() {
    let h = Home::new();
    let (_, launched) = h.run_agents(&["--all"], "\teeee5555\t", "", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.sub, "argv": ["codex", "resume", "eeee5555"]}));
    let (_, launched) = h.run_agents(&["--all"], "\thhhh8888\t", "", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.game, "argv": ["pi", "--session", "hhhh8888"]}));
    let (_, launched) = h.run_agents(&["--all"], "\taaaa\t", "", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.work, "argv": ["claude", "--resume", "aaaa"]}));
}

#[test]
fn ctrl_o_forks_and_ctrl_n_starts_anew() {
    let h = Home::new();
    let (_, launched) = h.run_agents(&["--all"], "\taaaa\t", "ctrl-o", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.work, "argv": ["claude", "--resume", "aaaa", "--fork-session"]}));
    let (_, launched) = h.run_agents(&["--all"], "\tdddd4444\t", "ctrl-o", "");
    assert_eq!(launched.unwrap()["argv"], json!(["codex", "fork", "dddd4444"]));
    let (_, launched) = h.run_agents(&["--all"], "\thhhh8888\t", "ctrl-o", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.game, "argv": ["pi", "--fork", "hhhh8888"]}));
    let (_, launched) = h.run_agents(&["--all"], "\teeee5555\t", "ctrl-n", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.sub, "argv": ["codex"]}));
}

#[test]
fn extra_arguments_go_to_the_agent() {
    let h = Home::new();
    let (_, launched) = h.run_agents(&["--all", "--", "--model", "x"], "\taaaa\t", "", "");
    assert_eq!(launched.unwrap()["argv"], json!(["claude", "--resume", "aaaa", "--model", "x"]));
}

#[test]
fn the_new_row_picks_a_folder() {
    let h = Home::new();
    let sibling = format!("{}/game2", h.tmp);
    let (_, launched) = h.run_agents(&["--all"], "new\tpi\t", "", &sibling);
    assert_eq!(launched.unwrap(), json!({"cwd": sibling, "argv": ["pi"]}));
    // a typed path wins over a row that merely matches it
    let (_, launched) = h.run_agents(&["--all"], "new\tcodex\t", "", "./");
    assert_eq!(launched.unwrap()["cwd"], json!(h.work));
}

#[test]
fn a_held_conversation_is_refused() {
    let h = Home::new();
    let sessions = format!("{}/sessions", h.claude);
    fs::create_dir_all(&sessions).unwrap();
    fs::write(format!("{sessions}/1.json"), dumps(&json!({"pid": std::process::id(), "sessionId": "aaaa"}), false)).unwrap();
    let (out, launched) = h.run_agents(&["--all"], "\taaaa\t", "", "");
    assert!(launched.is_none());
    assert!(!out.status.success());
    assert!(stderr(&out).contains("claude agents"));
    let (_, launched) = h.run_agents(&["--all"], "\taaaa\t", "ctrl-o", ""); // forking it is fine
    assert_eq!(launched.unwrap()["argv"][3], json!("--fork-session"));
}

#[test]
fn fzf_gets_the_switcher_bindings_and_the_list() {
    let h = Home::new();
    h.run_agents(&["--codex", "notes"], "", "", "");
    let calls = h.fzf_calls();
    let args = calls.last().unwrap();
    assert!(args.iter().any(|a| a == "ctrl-n,ctrl-o"));
    assert!(args.iter().any(|a| a.starts_with("tab:transform:") && a.ends_with("--cycle next footer")));
    assert!(args.iter().any(|a| a.starts_with("btab:transform:") && a.ends_with("--cycle back footer")));
    let right = args.iter().find(|a| a.starts_with("right:transform:")).unwrap();
    assert!(right.contains("[ -n \"$FZF_QUERY\" ] && echo forward-char")); // typing: the arrow moves in the text
    assert!(right.ends_with("--cycle next footer"));
    assert!(args.iter().find(|a| a.starts_with("left:transform:")).unwrap().ends_with("--cycle back footer"));
    assert_eq!(after(args, "--with-shell"), "sh -c");
    assert_eq!(after(args, "--query"), "notes");
    assert!(after(args, "--header").contains("\x1b[1;7m Codex "));
    assert!(after(args, "--footer").contains("←/→/Tab 切换列表"));
    assert!(after(args, "--footer").contains("Ctrl-X 归档（Codex）· Ctrl-D 删除"));
    assert!(args.iter().any(|a| a.starts_with("ctrl-x:transform:") && a.ends_with("--archive {1} {2} {3} {5} footer")));
    assert!(args.iter().any(|a| a.starts_with("ctrl-d:execute(") && a.contains("--delete {1} {2} {3} {4} {5})+transform:") && a.ends_with("--refresh footer")));
}

#[test]
fn older_fzf_gets_fewer_features() {
    let h = Home::new();
    let run = |version: &str| {
        fs::write(&h.log, "").unwrap();
        h.command(&["--codex"]).env("AGENTS_DRY_RUN", "1").env("FAKE_FZF_LOG", &h.log).env("FAKE_FZF_VERSION", version).output().unwrap();
        h.fzf_calls().pop().unwrap()
    };
    // no --footer: the key help rides in the header
    let args = run("0.60.0");
    assert!(!args.iter().any(|a| a == "--footer"));
    assert!(after(&args, "--header").contains("切换列表"));
    assert!(args.iter().any(|a| a.starts_with("tab:transform:") && a.ends_with("--cycle next inline")));
    // no --with-shell: the arrows ask this program, which reads $FZF_QUERY
    let args = run("0.50.0");
    assert!(!args.iter().any(|a| a == "--with-shell"));
    assert!(args.iter().any(|a| a.starts_with("right:transform:") && a.ends_with("--cycle next inline arrow")));
    // no transform: no switching at all, and it says so
    let args = run("0.40.0");
    assert!(!args.iter().any(|a| a.starts_with("tab:")));
    assert!(after(&args, "--header").contains("fzf 0.46 以上才能切换列表"));
    assert!(!args.iter().any(|a| a.starts_with("ctrl-x:") || a.starts_with("ctrl-d:")));
    assert!(!after(&args, "--header").contains("Ctrl-X"));
    assert!(!after(&args, "--header").contains("←/→"));
}

#[test]
fn this_folder_through_main() {
    let h = Home::new();
    let run = |pick: &str| {
        h.command(&[".", "--all"])
            .current_dir(&h.game)
            .env("AGENTS_DRY_RUN", "1")
            .env("FAKE_FZF_LOG", &h.log)
            .env("FAKE_FZF_PICK", pick)
            .output()
            .unwrap()
    };
    assert_eq!(String::from_utf8_lossy(&run("\tdddd4444\t").stdout).trim(), ""); // in work, not under game: not on the list
    let launched: Value = serde_json::from_slice(&run("\teeee5555\t").stdout).unwrap();
    assert_eq!(launched["cwd"], json!(h.sub));
}

#[test]
fn new_from_the_command_line() {
    let h = Home::new();
    let (_, launched) = h.run_agents(&["new", "codex", &h.game, "--", "--model", "x"], "", "", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.game, "argv": ["codex", "--model", "x"]}));
    let (_, launched) = h.run_agents(&["new", "pi", "../game"], "", "", "");
    assert_eq!(launched.unwrap(), json!({"cwd": h.game, "argv": ["pi"]}));
    let (out, launched) = h.run_agents(&["new", "claude", &format!("{}/nope", h.tmp)], "", "", "");
    assert!(launched.is_none());
    assert!(!out.status.success());
}

#[test]
fn an_empty_list_prints_nothing() {
    let h = Home::new();
    fs::remove_dir_all(&h.pi).unwrap();
    let out = h.command(&["--list", "pi"]).env("PATH", "/nonexistent").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

#[test]
fn update_fails_loudly() {
    let h = Home::new();
    h.script("curl", "#!/bin/sh\nexit 22\n");
    let (out, _) = h.run_agents(&["update"], "", "", "");
    assert!(!out.status.success());
    assert!(stderr(&out).contains("更新失败"));
}

#[test]
fn update_runs_the_installer_into_its_own_folder() {
    let h = Home::new();
    // this copy of the program, and a curl that hands over a little installer
    let copy = format!("{}/agents", h.bin);
    fs::copy(AGENTS_BIN, &copy).unwrap();
    h.script("curl", "#!/bin/sh\necho 'echo \"bin=$AGENTS_BIN_DIR\"; exit 7'\n");
    let out = retry_busy(|| Command::new(&copy).arg("update").envs(h.env()).env_remove("AGENTS_BIN_DIR").current_dir(&h.work).output());
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("bin={}", h.bin));
}

#[test]
fn reopens_the_last_list() {
    let h = Home::new();
    let state = format!("{}/agents", h.state);
    fs::create_dir_all(&state).unwrap();
    fs::write(format!("{state}/mode"), "codex").unwrap();
    h.run_agents(&[], "", "", "");
    assert!(after(h.fzf_calls().last().unwrap(), "--header").contains("\x1b[1;7m Codex "));
    fs::write(format!("{state}/mode"), b"\xff").unwrap(); // a damaged state file falls back to the mixed list
    h.run_agents(&[], "", "", "");
    assert!(after(h.fzf_calls().last().unwrap(), "--header").contains("\x1b[1;7m 全部 "));
}

#[test]
fn the_agent_really_starts_in_the_conversations_folder() {
    let h = Home::new();
    h.script("claude", "#!/bin/sh\necho \"pwd=$(pwd) args=$* here=${AGENTS_HERE:-none}\"\n");
    let out = h
        .command(&[".", "--all"])
        .current_dir(&h.game)
        .env("FAKE_FZF_LOG", &h.log)
        .env("FAKE_FZF_PICK", "\tbbbb\t")
        .env("FAKE_FZF_KEY", "")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("pwd={} args=--resume bbbb here=none", h.game));
}

#[test]
fn new_with_a_wrong_folder_or_argument() {
    let h = Home::new();
    let (out, launched) = h.run_agents(&["new", "cladue"], "", "", "");
    assert!(launched.is_none());
    assert!(stderr(&out).contains("claude、codex、pi"));
    let (out, launched) = h.run_agents(&["new", &h.game, "codex"], "", "", "");
    assert!(launched.is_none());
    assert!(stderr(&out).contains("用法"));
}

#[test]
fn a_folder_it_cannot_enter() {
    // SAFETY: geteuid has no preconditions
    if unsafe { libc::geteuid() } == 0 {
        return; // root enters any folder
    }
    let h = Home::new();
    let locked = format!("{}/locked", h.tmp);
    fs::create_dir_all(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let out = h.command(&["new", "claude", &locked]).output().unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!out.status.success());
    assert!(stderr(&out).contains("进不去这个目录"), "{}", stderr(&out));
    assert!(stderr(&out).contains("Permission denied"), "{}", stderr(&out));
}

#[test]
fn version() {
    let (out, _) = Home::new().run_agents(&["--version"], "", "", "");
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("agents "));
}

// ---------- small things the rest leans on ----------

#[test]
fn parse_takes_objects_only_and_survives_lone_surrogates() {
    assert!(parse(b"{\"a\":1}").is_some());
    assert!(parse(b"[1]").is_none());
    assert!(parse(b"\x00\xff").is_none());
    assert!(parse("{\"a\":".repeat(100_000).as_bytes()).is_none());
    assert_eq!(parse(br#"{"a":"x\ud83dy"}"#).unwrap()["a"], "x?y");
    assert_eq!(parse(br#"{"a":"x\ude00y"}"#).unwrap()["a"], "x?y");
    assert_eq!(parse(br#"{"a":"\ud83d\ude00"}"#).unwrap()["a"], "😀");
    // an escaped backslash before the letters is no surrogate, even in a line that needs taming
    assert_eq!(parse(br#"{"a":"\\ud83d \ud83d"}"#).unwrap()["a"], "\\ud83d ?");
}

#[test]
fn typed_tells_people_from_injections() {
    assert!(typed(Some("hello")));
    assert!(typed(Some("<div> a tag")));
    assert!(!typed(Some("  <system-reminder>x")));
    assert!(!typed(Some(" \n\t\u{3000}")));
    assert!(!typed(None));
}

#[test]
fn paths_behave_like_posix() {
    assert_eq!(normpath(""), ".");
    assert_eq!(normpath("/a/./b/../c/"), "/a/c");
    assert_eq!(normpath("//a"), "//a");
    assert_eq!(normpath("///a"), "/a");
    assert_eq!(normpath("/.."), "/");
    assert_eq!(normpath("a/../.."), "..");
    let h = Home::new();
    let link = format!("{}/link", h.tmp);
    std::os::unix::fs::symlink(&h.game, &link).unwrap();
    assert_eq!(realpath(&format!("{link}/sub")), h.sub);
    assert_eq!(realpath(&format!("{link}/not/there")), format!("{}/not/there", h.game));
    let looped = format!("{}/loop", h.tmp);
    std::os::unix::fs::symlink(&looped, &looped).unwrap();
    assert_eq!(realpath(&looped), looped);
}

// ---------- archiving and deleting ----------

impl Home {
    fn archived_ids(&self) -> Vec<String> {
        self.ids("archived")
    }

    /// `agents --archive` as fzf's Ctrl-X runs it: what fzf is told to do next.
    fn archive_key(&self, action: &str, agent: &str, ident: &str) -> String {
        let path = self.path_of(ident);
        let out = self.command(&["--archive", action, agent, ident, &path, "footer"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// `agents --delete` as fzf's Ctrl-D runs it, with `answer` typed at the question.
    fn delete_key(&self, action: &str, agent: &str, ident: &str, path: &str, answer: &str) -> Output {
        let mut child = self
            .command(&["--delete", action, agent, ident, &self.work, path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // a refusal comes before the question: the program may be gone before the answer is written
        let _ = child.stdin.take().unwrap().write_all(answer.as_bytes());
        child.wait_with_output().unwrap()
    }

    /// Where a conversation is now, on the lists or in the archive.
    fn path_of(&self, ident: &str) -> String {
        let ctx = self.ctx();
        ctx.conversations("all").into_iter().chain(ctx.conversations("archived")).find(|r| r.id == ident).map(|r| r.path).unwrap_or_default()
    }

    fn fake_codex(&self, code: i32) -> String {
        let log = format!("{}/codex.log", self.tmp);
        self.script("codex", &format!("#!/bin/sh\necho \"$*\" >> '{log}'\necho 'WARNING: noise' >&2\n[ {code} = 0 ] || echo 'Error: no such thread' >&2\nexit {code}\n"));
        log
    }
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

#[test]
fn codex_archives_with_its_own_commands() {
    let h = Home::new();
    let log = h.fake_codex(0);
    let out = h.archive_key("resume", "codex", "dddd4444");
    assert!(out.starts_with("reload("), "{out}");
    let path = h.files["dddd4444"].clone();
    h.delete_key("resume", "codex", "dddd4444", &path, "y\n");
    assert_eq!(fs::read_to_string(&log).unwrap(), "archive dddd4444\ndelete --force dddd4444\n");
    // what codex says when it fails is shown under the switcher; the list stays as it is
    h.fake_codex(1);
    let out = h.archive_key("resume", "codex", "eeee5555");
    assert!(out.starts_with("change-header:") && out.contains("归档失败：codex archive 出错：Error: no such thread"), "{out}");
    assert!(!out.contains("WARNING"));
}

#[test]
fn codex_archived_sessions_are_listed_and_taken_back_on_enter() {
    let h = Home::new();
    let archived = format!("{}/archived_sessions/rollout-2026-11-03T10-00-00-dddd4444.jsonl", h.codex);
    fs::create_dir_all(dirname(&archived)).unwrap();
    fs::rename(&h.files["dddd4444"], &archived).unwrap();
    let rows = h.ctx().rows_for("archived");
    assert_eq!(rows.len(), 1, "no new-conversation rows on the archived list: {rows:?}");
    assert!(rows[0].starts_with("archived\tcodex\tdddd4444\t") && rows[0].contains("[Codex]"));
    assert!(rows[0].contains("发布说明")); // its name from Codex's index still
    let log = h.fake_codex(0);
    let (_, launched) = h.run_agents(&["--archived"], "\tdddd4444\t", "", "");
    assert_eq!(launched.unwrap()["argv"], json!(["codex", "resume", "dddd4444"]));
    assert_eq!(fs::read_to_string(&log).unwrap(), "unarchive dddd4444\n");
}

#[test]
fn nothing_outside_the_agents_transcripts_is_touched() {
    let h = Home::new();
    let outside = format!("{}/outside.jsonl", h.tmp);
    fs::write(&outside, "keep me\n").unwrap();
    for path in [outside.clone(), format!("{}/../outside.jsonl", h.projects), format!("{}/sessions/../outside.jsonl", h.pi)] {
        h.delete_key("resume", "claude", "x", &path, "y\ny\n");
        h.delete_key("resume", "pi", "x", &path, "y\ny\n");
        let out = h.command(&["--archive", "resume", "codex", "x", &path, "footer"]).output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("这不是它的对话记录"));
    }
    let not_jsonl = format!("{}/-work/notes.txt", h.projects);
    fs::write(&not_jsonl, "x").unwrap();
    h.delete_key("resume", "claude", "x", &not_jsonl, "y\ny\n");
    assert!(exists(&outside) && exists(&not_jsonl));
    // a new-conversation row has nothing to archive or delete
    assert_eq!(h.command(&["--archive", "new", "claude", "", "", "footer"]).output().unwrap().stdout, b"");
    assert!(stderr(&h.delete_key("new", "claude", "", "", "y\n")).is_empty());
}

#[test]
fn refresh_shows_the_remembered_list() {
    let h = Home::new();
    h.ctx().remember("codex");
    let out = String::from_utf8_lossy(&h.command(&["--refresh", "inline"]).output().unwrap().stdout).into_owned();
    assert!(out.starts_with("reload(") && out.contains("--list codex") && out.contains("Ctrl-X 归档"), "{out}");
}

#[test]
fn claude_and_pi_have_no_archive_and_agents_makes_none() {
    let h = Home::new();
    for (agent, ident, name) in [("claude", "bbbb", "Claude"), ("pi", "hhhh8888", "Pi")] {
        let out = h.archive_key("resume", agent, ident);
        assert!(out.starts_with("change-header:") && out.contains(&format!("{name} 自己没有归档")) && out.contains("Ctrl-D 删除"), "{out}");
    }
    // nothing moved, nothing made
    assert!(exists(&h.files["bbbb"]) && exists(&h.files["hhhh8888"]) && exists(&format!("{}/-game/bbbb/subagents/sub1.jsonl", h.projects)));
    assert!(!exists(&format!("{}/agents-archive", h.claude)) && !exists(&format!("{}/agents-archive", h.pi)));
    assert!(h.archived_ids().is_empty());
    assert_eq!(h.ctx().lists(), ["all", "claude", "codex", "pi"]);
}

#[test]
fn the_archived_list_holds_what_codex_archived() {
    let h = Home::new();
    let ctx = h.ctx();
    ctx.remember("archived");
    assert_eq!(ctx.remembered(), "all"); // nothing archived: no such list
    let archived = format!("{}/archived_sessions/rollout-2026-11-03T10-00-00-dddd4444.jsonl", h.codex);
    fs::create_dir_all(dirname(&archived)).unwrap();
    fs::rename(&h.files["dddd4444"], &archived).unwrap();
    assert_eq!(ctx.lists(), ["all", "claude", "codex", "pi", "archived"]);
    ctx.remember("pi");
    assert!(ctx.cycle(1, true, false, None).contains("--list archived"));
    assert!(ctx.header("archived", true).contains("Enter 取回并接着聊"));
    assert!(!ctx.header("all", true).contains("取回"));
    // Ctrl-X there asks Codex to put it back; Ctrl-D asks Codex to delete it
    let log = h.fake_codex(0);
    assert!(h.archive_key("archived", "codex", "dddd4444").starts_with("reload("));
    h.delete_key("archived", "codex", "dddd4444", &archived, "y\n");
    assert_eq!(fs::read_to_string(&log).unwrap(), "unarchive dddd4444\ndelete --force dddd4444\n");
    // once the archive is empty the list goes, back to the mixed one
    fs::remove_file(&archived).unwrap();
    assert!(h.archive_key("resume", "codex", "eeee5555").contains("--list all"));
    assert_eq!(ctx.remembered(), "all");
}

#[test]
fn a_held_conversation_is_neither_archived_nor_deleted() {
    let h = Home::new();
    let sessions = format!("{}/sessions", h.claude);
    fs::create_dir_all(&sessions).unwrap();
    fs::write(format!("{sessions}/1.json"), dumps(&json!({"pid": std::process::id(), "sessionId": "aaaa"}), false)).unwrap();
    let out = h.delete_key("resume", "claude", "aaaa", &h.files["aaaa"], "y\n\n");
    assert!(stderr(&out).contains("这个对话正被另一个 Claude 进程占着，不能删除"), "{}", stderr(&out));
    assert!(exists(&h.files["aaaa"]));
    // a codex that resumed a conversation holds it: it can be neither archived nor deleted. The id is
    // this test's own: tests running alongside read the same process table.
    let mut h = h;
    let (w, day) = (h.work.clone(), h.codex_day.clone());
    h.add("held7777", &format!("{day}/rollout-2026-11-03T14-00-00-held7777.jsonl"), &[
        json!({"type": "session_meta", "payload": {"id": "held7777", "cwd": w, "source": "cli"}}),
        codex_user(json!("still running")),
    ], false);
    let codex = format!("{}/fakes/codex", h.tmp);
    fs::create_dir_all(dirname(&codex)).unwrap();
    fs::copy(fs::canonicalize("/bin/sh").unwrap(), &codex).unwrap();
    let mut running = retry_busy(|| Command::new(&codex).args(["-c", "sleep 30; :", "resume", "held7777"]).spawn());
    std::thread::sleep(Duration::from_millis(200));
    let log = h.fake_codex(0);
    let archived = h.archive_key("resume", "codex", "held7777");
    let deleted = h.delete_key("resume", "codex", "held7777", &h.files["held7777"], "y\n\n");
    running.kill().unwrap();
    running.wait().unwrap();
    assert!(archived.contains("这个对话正被另一个 Codex 进程占着，不能归档"), "{archived}");
    assert!(stderr(&deleted).contains("不能删除"));
    assert!(!exists(&log), "codex was not asked to do anything");
}

#[test]
fn ctrl_d_deletes_only_after_a_yes() {
    let h = Home::new();
    let path = h.files["bbbb"].clone();
    let out = h.delete_key("resume", "claude", "bbbb", &path, "n\n");
    assert!(stderr(&out).contains("永久删除这个 Claude 对话") && stderr(&out).contains("Pinball"), "{}", stderr(&out));
    assert!(exists(&path));
    h.delete_key("resume", "claude", "bbbb", &path, "\n");
    assert!(exists(&path));
    let out = h.delete_key("resume", "claude", "bbbb", &path, "y\n");
    assert!(out.status.success());
    // the transcript and the folder of subagents beside it
    assert!(!exists(&path) && !exists(&format!("{}/-game/bbbb", h.projects)));
    assert!(!h.ids("all").contains(&"bbbb".to_string()));
    // Pi: the transcript, as Pi's own picker deletes it
    h.delete_key("resume", "pi", "hhhh8888", &h.files["hhhh8888"], "y\n");
    assert!(!exists(&h.files["hhhh8888"]) && h.ids("pi").is_empty());
}

/// Runs a program just written: another test's fork may hold it open for a moment (ETXTBSY).
fn retry_busy<T>(mut run: impl FnMut() -> std::io::Result<T>) -> T {
    for _ in 0..50 {
        match run() {
            Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(Duration::from_millis(20)),
            other => return other.unwrap(),
        }
    }
    run().unwrap()
}

fn dirname(path: &str) -> String {
    std::path::Path::new(path).parent().unwrap().to_string_lossy().into_owned()
}

#[test]
fn a_pi_running_in_the_folder_keeps_its_conversations_in_place() {
    if !std::path::Path::new("/proc/self/comm").exists() {
        return; // the folders of processes are read from /proc
    }
    let h = Home::new();
    // Pi names its process `pi` and nothing more; a copy of sh called pi, waiting, does the same
    let pi = format!("{}/fakes/pi", h.tmp);
    fs::create_dir_all(dirname(&pi)).unwrap();
    fs::copy(fs::canonicalize("/bin/sh").unwrap(), &pi).unwrap();
    let mut running = retry_busy(|| Command::new(&pi).args(["-c", "sleep 30; :"]).current_dir(&h.game).spawn());
    std::thread::sleep(Duration::from_millis(200));
    let deleted = h.delete_key("resume", "pi", "hhhh8888", &h.files["hhhh8888"], "y\n\n");
    running.kill().unwrap();
    running.wait().unwrap();
    assert!(stderr(&deleted).contains("这个目录里有 Pi 在运行，可能正开着这个对话，不能删除"), "{}", stderr(&deleted));
    assert!(exists(&h.files["hhhh8888"]));
    // once it is gone, it can go
    h.delete_key("resume", "pi", "hhhh8888", &h.files["hhhh8888"], "y\n");
    assert!(!exists(&h.files["hhhh8888"]));
}
