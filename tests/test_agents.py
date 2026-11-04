"""agents against made-up Claude Code, Codex and Pi homes. Run: python3 -m unittest discover -s tests"""
import importlib.machinery
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(ROOT, "agents")

FAKE_FZF = r'''#!/usr/bin/env python3
# stands in for fzf: picks the first row holding $FAKE_FZF_PICK, presses $FAKE_FZF_KEY, types $FAKE_FZF_QUERY
import json, os, sys
args = sys.argv[1:]
if args == ["--version"]:
    print("0.74.5 (fake)")
    sys.exit(0)
with open(os.environ["FAKE_FZF_LOG"], "a") as f:
    f.write(json.dumps(args) + "\n")
rows = sys.stdin.read().split("\n")
pick = os.environ.get("FAKE_FZF_PICK", "")
chosen = next((r for r in rows if pick and pick in r), "")
out = []
if "--print-query" in args:
    out.append(os.environ.get("FAKE_FZF_QUERY", ""))
if "--expect" in args:
    out.append(os.environ.get("FAKE_FZF_KEY", ""))
if chosen:
    out.append(chosen)
print("\n".join(out))
sys.exit(0 if chosen else 1)
'''


def load(env):
    """A fresh copy of the agents module, reading the homes in `env`."""
    with mock.patch.dict(os.environ, env):
        loader = importlib.machinery.SourceFileLoader("agents_under_test", SCRIPT)
        spec = importlib.util.spec_from_loader("agents_under_test", loader)
        module = importlib.util.module_from_spec(spec)
        loader.exec_module(module)
        return module


def write_jsonl(path, rows, compact=False):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for row in rows:
            if isinstance(row, str):
                f.write(row + "\n")
            else:
                f.write(json.dumps(row, ensure_ascii=False, separators=(",", ":") if compact else None) + "\n")


def user(cwd, content, **extra):
    return {"type": "user", "cwd": cwd, "message": {"role": "user", "content": content}, **extra}


def codex_user(text_or_blocks):
    content = text_or_blocks if isinstance(text_or_blocks, list) else [{"type": "input_text", "text": text_or_blocks}]
    return {"type": "response_item", "payload": {"type": "message", "role": "user", "content": content}}


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        t = os.path.realpath(self.tmp.name)  # macOS: /var/folders is /private/var/folders
        self.claude = os.path.join(t, "claude")
        self.codex = os.path.join(t, "codex")
        self.pi = os.path.join(t, "pi")
        self.work = os.path.join(t, "work")
        self.game = os.path.join(t, "game")
        self.sub = os.path.join(self.game, "sub")
        self.sibling = os.path.join(t, "game2")
        for d in (self.work, self.sub, self.sibling):
            os.makedirs(d)
        self.env = {"CLAUDE_CONFIG_DIR": self.claude, "CODEX_HOME": self.codex, "PI_CODING_AGENT_DIR": self.pi,
                    "PI_CODING_AGENT_SESSION_DIR": "", "XDG_STATE_HOME": os.path.join(t, "state")}
        self.env.pop("PI_CODING_AGENT_SESSION_DIR")
        self.claude_projects = os.path.join(self.claude, "projects")
        self.codex_day = os.path.join(self.codex, "sessions", "2026", "11", "03")
        self.pi_game = os.path.join(self.pi, "sessions", "--game--")
        self.files = {}

        self.add("claude", "aaaa", os.path.join(self.claude_projects, "-work", "aaaa.jsonl"), [
            {"type": "permission-mode", "permissionMode": "default"},
            user(self.work, "<local-command-caveat>x</local-command-caveat>", isMeta=True),
            user(self.work, "fix the login bug"),
            {"type": "assistant", "cwd": self.work, "message": {"role": "assistant", "content": [{"type": "text", "text": "Fixed it."}]}},
            {"type": "ai-title", "aiTitle": "Login bug"},
            {"type": "custom-title", "customTitle": "登录问题"},
        ])
        self.add("claude", "bbbb", os.path.join(self.claude_projects, "-game", "bbbb.jsonl"), [
            user(self.game, [{"type": "text", "text": "make a pinball game"}]),
            {"type": "ai-title", "aiTitle": "Pinball"},
        ], compact=True)
        self.add("claude", "cccc", os.path.join(self.claude_projects, "-game", "cccc.jsonl"), [{"type": "mode", "mode": "normal"}])
        self.add("claude", "sub1", os.path.join(self.claude_projects, "-game", "bbbb", "subagents", "sub1.jsonl"), [
            user(self.game, "subagent work"),
        ])
        self.add("codex", "dddd4444", os.path.join(self.codex_day, "rollout-2026-11-03T10-00-00-dddd4444.jsonl"), [
            {"type": "session_meta", "payload": {"id": "dddd4444", "cwd": self.work, "source": "cli"}},
            codex_user("<environment_context>cwd</environment_context>"),
            codex_user("write the release notes"),
            {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done."}]}},
        ])
        self.add("codex", "eeee5555", os.path.join(self.codex_day, "rollout-2026-11-03T11-00-00-eeee5555.jsonl"), [
            {"type": "session_meta", "payload": {"id": "eeee5555", "cwd": self.sub, "source": "vscode"}},
            codex_user("add a score board"),
        ])
        self.add("codex", "ffff6666", os.path.join(self.codex_day, "rollout-2026-11-03T12-00-00-ffff6666.jsonl"), [
            {"type": "session_meta", "payload": {"id": "ffff6666", "cwd": self.work, "source": {"subagent": {"other": "guardian"}}}},
            codex_user("review this command"),
        ])
        write_jsonl(os.path.join(self.codex, "session_index.jsonl"), [
            {"id": "dddd4444", "thread_name": "old name"},
            {"id": "dddd4444", "thread_name": "发布说明"},
        ])
        self.add("pi", "hhhh8888", os.path.join(self.pi_game, "2026-11-01T09-00-00-000Z_hhhh8888.jsonl"), [
            {"type": "session", "version": 3, "id": "hhhh8888", "cwd": self.game},
            {"type": "message", "id": "a1", "parentId": None, "message": {"role": "user", "content": "tune the flippers"}},
            {"type": "message", "id": "a2", "parentId": "a1", "message": {"role": "assistant", "content": [{"type": "text", "text": "Tuned."}]}},
            {"type": "session_info", "id": "a3", "parentId": "a2", "name": "弹珠手感"},
        ])
        self.add("pi", "iiii9999", os.path.join(self.pi_game, "2026-11-01T08-00-00-000Z_iiii9999.jsonl"), [
            {"type": "session", "version": 3, "id": "iiii9999", "cwd": self.game},
        ])
        # newest first: eeee5555, bbbb, dddd4444, aaaa, hhhh8888
        for i, name in enumerate(["hhhh8888", "aaaa", "dddd4444", "bbbb", "eeee5555"]):
            os.utime(self.files[name], (1_800_000_000 + i * 60, 1_800_000_000 + i * 60))
        self.cr = load(self.env)

    def add(self, agent, ident, path, rows, compact=False):
        write_jsonl(path, rows, compact)
        self.files[ident] = path

    def tearDown(self):
        self.tmp.cleanup()

    def ids(self, mode="all"):
        return [r["id"] for r in self.cr.conversations(mode)]

    def titles(self, mode="all"):
        return {r["id"]: r["title"] for r in self.cr.conversations(mode)}


class Reading(Base):
    def test_lists_every_agent_newest_first(self):
        self.assertEqual(self.ids(), ["eeee5555", "bbbb", "dddd4444", "aaaa", "hhhh8888"])

    def test_titles(self):
        titles = self.titles()
        self.assertEqual(titles["aaaa"], "登录问题")  # /rename beats the AI title
        self.assertEqual(titles["bbbb"], "Pinball")
        self.assertEqual(titles["dddd4444"], "发布说明")  # the last name in Codex's index
        self.assertEqual(titles["eeee5555"], "add a score board")  # unnamed: the first thing typed
        self.assertEqual(titles["hhhh8888"], "弹珠手感")  # Pi's /name

    def test_one_agent(self):
        self.assertEqual(self.ids("codex"), ["eeee5555", "dddd4444"])
        self.assertEqual(self.ids("claude"), ["bbbb", "aaaa"])
        self.assertEqual(self.ids("pi"), ["hhhh8888"])

    def test_skips_empty_conversations_and_helpers(self):
        ids = set(self.ids())
        for left_out in ("cccc", "sub1", "ffff6666", "iiii9999"):
            self.assertNotIn(left_out, ids)

    def test_cwd_comes_from_the_transcript(self):
        cwd = {r["id"]: r["cwd"] for r in self.cr.conversations()}
        self.assertEqual(cwd["aaaa"], self.work)
        self.assertEqual(cwd["eeee5555"], self.sub)

    def test_a_prompt_after_injected_blocks(self):
        # Codex stores an attached image as marker blocks around it; Claude puts an IDE selection first
        self.add("codex", "img1", os.path.join(self.codex_day, "rollout-2026-11-03T13-00-00-img1.jsonl"), [
            {"type": "session_meta", "payload": {"id": "img1", "cwd": self.work, "source": "cli"}},
            codex_user([{"type": "input_text", "text": "<image name=[Image #1]>"}, {"type": "input_image", "image_url": "data:"},
                        {"type": "input_text", "text": "</image>"}, {"type": "input_text", "text": "[Image #1] what can you see"}]),
        ])
        self.add("claude", "ide1", os.path.join(self.claude_projects, "-work", "ide1.jsonl"), [
            user(self.work, [{"type": "text", "text": "<ide_selection>x = 1</ide_selection>"}, {"type": "text", "text": "typed question"}]),
        ])
        titles = self.titles()
        self.assertEqual(titles["img1"], "[Image #1] what can you see")
        self.assertEqual(titles["ide1"], "typed question")

    def test_a_prompt_that_starts_with_a_tag_is_still_a_prompt(self):
        self.add("claude", "tag1", os.path.join(self.claude_projects, "-work", "tag1.jsonl"), [user(self.work, "<div> why is this broken")])
        self.assertEqual(self.titles()["tag1"], "<div> why is this broken")

    def test_a_conversation_started_by_a_command(self):
        command = "<command-name>/review</command-name>\n<command-message>review</command-message>\n<command-args>PR 7</command-args>"
        self.add("claude", "cmd1", os.path.join(self.claude_projects, "-work", "cmd1.jsonl"), [
            user(self.work, command),
            {"type": "assistant", "cwd": self.work, "message": {"model": "claude-opus-5-5", "role": "assistant", "content": [{"type": "text", "text": "Reviewing."}]}},
        ])
        # a command that changed a setting and got no answer is no conversation to resume
        self.add("claude", "cmd2", os.path.join(self.claude_projects, "-work", "cmd2.jsonl"), [
            user(self.work, "<command-name>/model</command-name>"),
            {"type": "assistant", "cwd": self.work, "message": {"model": "<synthetic>", "role": "assistant", "content": [{"type": "text", "text": "No response requested."}]}},
        ])
        titles = self.titles()
        self.assertEqual(titles["cmd1"], "/review PR 7")
        self.assertNotIn("cmd2", titles)

    def test_typed_text_after_many_tool_results(self):
        rows = [user(self.work, "<command-name>/init</command-name>")]
        rows += [user(self.work, [{"type": "tool_result", "tool_use_id": str(i), "content": "ok"}]) for i in range(400)]
        rows += [user(self.work, "and now the tests")]
        self.add("claude", "late1", os.path.join(self.claude_projects, "-work", "late1.jsonl"), rows, compact=True)
        self.assertEqual(self.titles()["late1"], "and now the tests")

    def test_broken_files_are_skipped_not_fatal(self):
        bad = os.path.join(self.claude_projects, "-bad")
        # a lone surrogate escape, as JavaScript writes a string cut mid-emoji
        write_jsonl(os.path.join(bad, "s1.jsonl"), [json.dumps(user(self.work, "broken \ud83d emoji"))])
        write_jsonl(os.path.join(bad, "s2.jsonl"), [{"type": "user", "cwd": self.work, "message": "just a string"}])
        write_jsonl(os.path.join(bad, "s3.jsonl"), ['{"type":"user","cwd":"%s","message":%s' % (self.work, "[" * 100_000)])
        write_jsonl(os.path.join(bad, "s4.jsonl"), [user(7, "cwd is a number")])
        with open(os.path.join(bad, "s5.jsonl"), "wb") as f:
            f.write(b"\x00\xff garbage\n\x89PNG")
        write_jsonl(os.path.join(self.codex_day, "rollout-x-bad1.jsonl"), [{"type": "session_meta", "payload": {"id": ["x"], "cwd": 3}}])
        write_jsonl(os.path.join(self.codex_day, "rollout-x-bad2.jsonl"), [[1, 2, 3]])
        write_jsonl(os.path.join(self.pi_game, "bad.jsonl"), [{"type": "session", "id": {"x": 1}, "cwd": self.game}])
        with open(os.path.join(self.codex, "session_index.jsonl"), "a") as f:
            f.write(json.dumps({"id": ["x"], "thread_name": "y"}) + "\n")
        os.makedirs(os.path.join(self.claude, "sessions"), exist_ok=True)
        with open(os.path.join(self.claude, "sessions", "9.json"), "w") as f:
            json.dump({"pid": os.getpid(), "sessionId": ["x"]}, f)
        ids = self.ids()
        self.assertIn("aaaa", ids)
        self.assertIn("s1", ids)  # a lone surrogate is still a conversation
        res = subprocess.run([sys.executable, "-I", SCRIPT, "--list", "all"], capture_output=True, env={**os.environ, **self.env})
        self.assertEqual(res.returncode, 0, res.stderr)
        self.assertIn(b"broken", res.stdout)

    def test_more_titles_and_prompts(self):
        self.add("claude", "named1", os.path.join(self.claude_projects, "-work", "named1.jsonl"), [
            user(self.work, "hello"), {"type": "agent-name", "agentName": "后台任务"},
        ])
        self.add("claude", "tag2", os.path.join(self.claude_projects, "-work", "tag2.jsonl"), [user(self.work, "<image> tag has no alt")])
        self.add("claude", "compact1", os.path.join(self.claude_projects, "-work", "compact1.jsonl"), [
            user(self.work, "This session is being continued from a previous conversation", isCompactSummary=True),
        ])
        self.add("claude", "cwd2", os.path.join(self.claude_projects, "-work", "cwd2.jsonl"), [
            '{"type":"x","cwd":"/tmp", TRUNCATED', user(self.work, "after a broken line"),
        ], compact=True)
        self.add("claude", "cr1", os.path.join(self.claude_projects, "-work", "cr1.jsonl"), [user(self.work + "\rb", "carriage return folder")])
        titles = self.titles()
        self.assertEqual(titles["named1"], "后台任务")
        self.assertEqual(titles["tag2"], "<image> tag has no alt")
        self.assertNotIn("compact1", titles)  # a compaction summary is nothing the person said
        self.assertEqual(titles["cwd2"], "after a broken line")
        self.assertNotIn("cr1", titles)

    def test_pi_session_folder_settings(self):
        elsewhere = os.path.join(self.tmp.name, "pi-elsewhere")
        shutil.move(os.path.join(self.pi, "sessions"), elsewhere)
        with open(os.path.join(self.pi, "settings.json"), "w") as f:
            json.dump({"sessionDir": elsewhere}, f)
        self.assertEqual(load(self.env).PI_SESSIONS, elsewhere)
        self.assertIn("hhhh8888", [r["id"] for r in load(self.env).conversations("pi")])
        other = os.path.join(self.tmp.name, "pi-env")
        self.assertEqual(load({**self.env, "PI_CODING_AGENT_SESSION_DIR": other}).PI_SESSIONS, other)

    def test_this_folder_and_below_only(self):
        with mock.patch.dict(os.environ, {"AGENTS_HERE": self.game}):
            cwds = {r["cwd"] for r in self.cr.conversations()}
        self.assertEqual(cwds, {self.game, self.sub})  # not the sibling game2, not work


class Held(Base):
    def test_claude_by_session_record(self):
        sessions = os.path.join(self.claude, "sessions")
        os.makedirs(sessions)
        with open("/proc/self/stat") if os.path.exists("/proc/self/stat") else open(os.devnull) as f:
            stat = f.read()
        started = stat[stat.rindex(")") + 2:].split()[19] if stat else None
        records = {"1": {"pid": os.getpid(), "sessionId": "aaaa", "procStart": started},
                   "2": {"pid": 999_999_999, "sessionId": "bbbb"}}
        if started:
            # the same pid, but a process started at another time: the record is stale
            records["3"] = {"pid": os.getpid(), "sessionId": "zzzz", "procStart": "1"}
        for name, record in records.items():
            with open(os.path.join(sessions, name + ".json"), "w") as f:
                json.dump(record, f)
        self.assertEqual(self.cr.held_claude(), {"aaaa"})

    def test_lock_holders_reads_proc_locks(self):
        locks = os.path.join(self.tmp.name, "locks")
        with open(locks, "w") as f:
            f.write("48: FLOCK  ADVISORY  WRITE 1009782 08:01:786440 0 EOF\n"
                    "48: -> FLOCK  ADVISORY  WRITE 1009999 08:01:786440 0 EOF\n"
                    "49: POSIX  ADVISORY  READ 77 00:2a:12 0 EOF\n")
        self.assertEqual(self.cr.lock_holders(locks), {786440: {1009782}, 12: {77}})

    def test_codex_by_writer_lock_held_by_codex(self):
        locks = os.path.join(self.codex, "thread-writer-locks")
        os.makedirs(locks)
        for name in ("dddd4444.lock", "eeee5555.lock", ".coordination.lock"):
            open(os.path.join(locks, name), "w").close()
        inode = lambda name: os.stat(os.path.join(locks, name)).st_ino
        rows = self.cr.conversations("codex")
        argv = {100: ["/usr/bin/codex"], 200: ["vim", "notes.txt"]}
        with mock.patch.object(self.cr, "command_line", lambda pid: argv.get(pid, [])):
            # codex holds dddd4444's lock; a vim holding an inode that happens to match eeee5555's is no codex
            self.assertEqual(self.cr.held_codex(rows, [], {inode("dddd4444.lock"): {100}, inode("eeee5555.lock"): {200}}), {"dddd4444"})

    def test_codex_and_pi_by_command_line(self):
        rows = self.cr.conversations()
        procs = [["codex", "-c", "x=1", "resume", "eeee5555"],
                 ["codex", "fork", "dddd4444"],  # a fork holds its new conversation, not the source
                 ["vim", "resume", "dddd4444"],
                 ["node", "/home/u/.local/bin/pi", "--session", "hhhh8888"]]
        self.assertEqual(self.cr.held_by_command_line(rows, "codex", procs), {"eeee5555"})
        self.assertEqual(self.cr.held_by_command_line(rows, "pi", procs), {"hhhh8888"})
        self.assertEqual(self.cr.held_by_command_line(rows, "pi", [["node", "/x/pi-coding-agent/dist/cli.js", "--fork", "hhhh8888"]]), set())
        self.assertEqual(self.cr.held_by_command_line(rows, "pi", [["pi", "--session=hhhh8888"]]), {"hhhh8888"})
        self.assertEqual(self.cr.held_by_command_line(rows, "pi", [["pi", "--session-id", "hhhh8888"]]), {"hhhh8888"})


class Lists(Base):
    def test_tab_goes_through_every_present_agent_and_shift_tab_back(self):
        for nxt in ("claude", "codex", "pi", "all", "claude"):
            self.assertIn(f"--list {nxt}", self.cr.cycle(1))
            self.assertEqual(self.cr.remembered(), nxt)
        self.assertIn("--list all", self.cr.cycle(-1))
        self.assertIn("--list pi", self.cr.cycle(-1))

    def test_the_switcher_highlights_the_list_shown(self):
        out = self.cr.cycle(1)  # all -> claude
        self.assertTrue(out.split("+")[-1].startswith("change-header:"))  # takes the rest of the line: last
        bar = self.cr.switcher("codex")
        for label in ("全部", "Claude", "Codex", "Pi"):
            self.assertIn(f" {label} ", bar)
        self.assertIn("\033[1;7m Codex ", bar)  # reverse video on the list shown
        self.assertNotIn("\033[1;7m Claude ", bar)
        self.assertIn("切换列表", self.cr.header("all", footer=False))  # no footer: key help rides along
        self.assertNotIn("切换列表", self.cr.header("all", footer=True))

    def test_cycle_skips_an_agent_with_nothing_here(self):
        shutil.rmtree(self.pi)
        self.cr.remember("codex")
        with mock.patch.object(self.cr.shutil, "which", lambda name: None):
            self.assertIn("--list all", self.cr.cycle(1))
            self.assertNotIn(" Pi ", self.cr.switcher("all"))
        # a folder with no conversation files does not count either
        os.makedirs(os.path.join(self.pi, "sessions"))
        with mock.patch.object(self.cr.shutil, "which", lambda name: None):
            self.assertNotIn("pi", self.cr.lists())

    def test_remembers_the_last_list(self):
        self.assertEqual(self.cr.remembered(), "all")
        self.cr.cycle(1)
        self.cr.cycle(1)
        self.assertEqual(self.load_again().remembered(), "codex")

    def load_again(self):
        return load(self.env)

    def shown(self, mode):
        return {r.split("\t")[2]: r.split("\t")[5] for r in self.cr.rows_for(mode) if r.startswith("resume")}

    def test_the_mixed_list_names_each_agent(self):
        shown = self.shown("all")
        self.assertIn("[Claude]", shown["aaaa"])
        self.assertIn("[Codex]", shown["dddd4444"])
        self.assertIn("[Pi]", shown["hhhh8888"])
        self.assertNotIn("[Codex]", self.shown("codex")["dddd4444"])

    def test_rows_carry_tab_separated_fields(self):
        rows = [r.split("\t") for r in self.cr.rows_for("all") if r.startswith("resume")]
        self.assertTrue(all(len(r) == 6 for r in rows))
        self.assertEqual(rows[0][1:4], ["codex", "eeee5555", self.sub])

    def test_the_first_prompt_is_searchable(self):
        self.assertIn("fix the login bug", self.shown("all")["aaaa"])
        long = "x" * 200
        self.add("claude", "long1", os.path.join(self.claude_projects, "-work", "long1.jsonl"), [user(self.work, long)])
        self.assertIn(long, self.shown("claude")["long1"])  # the title shows 140, the rest stays searchable

    def test_new_rows_for_installed_agents(self):
        with mock.patch.object(self.cr.shutil, "which", lambda name: "/bin/" + name if name == "codex" else None):
            news = [r.split("\t")[1] for r in self.cr.rows_for("all") if r.startswith("new")]
        self.assertEqual(news, ["codex"])

    def test_pad_counts_wide_characters(self):
        self.assertEqual(self.cr.width(self.cr.pad("登录", 6)), 6)
        self.assertEqual(self.cr.width(self.cr.pad("~/a/very/long/folder/name", 10)), 10)
        self.assertNotIn("…", self.cr.pad(self.cr.ago(0) if False else "59分钟前", 8))

    def preview(self, agent, ident):
        return subprocess.run([sys.executable, "-I", SCRIPT, "--preview", agent, self.files[ident], ident, self.work],
                              capture_output=True, text=True, env={**os.environ, **self.env}).stdout

    def test_preview_of_each_agent(self):
        out = self.preview("codex", "dddd4444")
        self.assertIn("write the release notes", out)
        self.assertIn("Done.", out)
        self.assertNotIn("environment_context", out)
        self.assertIn("make a pinball game", self.preview("claude", "bbbb"))  # a one-line file keeps its first line
        self.assertIn("Tuned.", self.preview("pi", "hhhh8888"))

    def test_preview_looks_further_back(self):
        rows = [user(self.work, "the early question")]
        rows += [user(self.work, [{"type": "tool_result", "tool_use_id": str(i), "content": "y" * 1000}]) for i in range(900)]
        self.add("claude", "deep1", os.path.join(self.claude_projects, "-work", "deep1.jsonl"), rows, compact=True)
        self.assertIn("the early question", self.preview("claude", "deep1"))


class Commands(Base):
    def setUp(self):
        super().setUp()
        self.bin = os.path.join(self.tmp.name, "bin")
        os.makedirs(self.bin)
        for name, body in (("claude", "#!/bin/sh\n"), ("codex", "#!/bin/sh\n"), ("pi", "#!/bin/sh\n"), ("fzf", FAKE_FZF)):
            path = os.path.join(self.bin, name)
            with open(path, "w") as f:
                f.write(body.replace("#!/usr/bin/env python3", "#!" + sys.executable))
            os.chmod(path, 0o755)
        self.log = os.path.join(self.tmp.name, "fzf.log")

    def run_agents(self, *args, pick="", key="", query="", extra_env=None):
        env = {**os.environ, **self.env, "AGENTS_DRY_RUN": "1", "PATH": self.bin + os.pathsep + os.environ.get("PATH", ""),
               "FAKE_FZF_LOG": self.log, "FAKE_FZF_PICK": pick, "FAKE_FZF_KEY": key, "FAKE_FZF_QUERY": query, **(extra_env or {})}
        res = subprocess.run([sys.executable, "-I", SCRIPT, *args], capture_output=True, text=True, env=env, cwd=self.work)
        launched = json.loads(res.stdout) if res.stdout.strip().startswith("{") else None
        return res, launched

    def fzf_calls(self):
        with open(self.log) as f:
            return [json.loads(line) for line in f]

    def test_enter_resumes_in_the_conversations_folder(self):
        _, launched = self.run_agents("--all", pick="\teeee5555\t")
        self.assertEqual(launched, {"cwd": self.sub, "argv": ["codex", "resume", "eeee5555"]})
        _, launched = self.run_agents("--all", pick="\thhhh8888\t")
        self.assertEqual(launched, {"cwd": self.game, "argv": ["pi", "--session", "hhhh8888"]})
        _, launched = self.run_agents("--all", pick="\taaaa\t", extra_env=None)
        self.assertEqual(launched, {"cwd": self.work, "argv": ["claude", "--resume", "aaaa"]})

    def test_ctrl_o_forks_and_ctrl_n_starts_anew(self):
        _, launched = self.run_agents("--all", pick="\taaaa\t", key="ctrl-o")
        self.assertEqual(launched, {"cwd": self.work, "argv": ["claude", "--resume", "aaaa", "--fork-session"]})
        _, launched = self.run_agents("--all", pick="\tdddd4444\t", key="ctrl-o")
        self.assertEqual(launched["argv"], ["codex", "fork", "dddd4444"])
        _, launched = self.run_agents("--all", pick="\thhhh8888\t", key="ctrl-o")
        self.assertEqual(launched, {"cwd": self.game, "argv": ["pi", "--fork", "hhhh8888"]})
        _, launched = self.run_agents("--all", pick="\teeee5555\t", key="ctrl-n")
        self.assertEqual(launched, {"cwd": self.sub, "argv": ["codex"]})

    def test_extra_arguments_go_to_the_agent(self):
        _, launched = self.run_agents("--all", "--", "--model", "x", pick="\taaaa\t")
        self.assertEqual(launched["argv"], ["claude", "--resume", "aaaa", "--model", "x"])

    def test_the_new_row_picks_a_folder(self):
        _, launched = self.run_agents("--all", pick="new\tpi\t", query=self.sibling)
        self.assertEqual(launched, {"cwd": self.sibling, "argv": ["pi"]})
        # a typed path wins over a row that merely matches it
        _, launched = self.run_agents("--all", pick="new\tcodex\t", query="./")
        self.assertEqual(launched["cwd"], self.work)

    def test_a_held_conversation_is_refused(self):
        sessions = os.path.join(self.claude, "sessions")
        os.makedirs(sessions)
        with open(os.path.join(sessions, "1.json"), "w") as f:
            json.dump({"pid": os.getpid(), "sessionId": "aaaa"}, f)
        res, launched = self.run_agents("--all", pick="\taaaa\t")
        self.assertIsNone(launched)
        self.assertNotEqual(res.returncode, 0)
        self.assertIn("claude agents", res.stderr)
        _, launched = self.run_agents("--all", pick="\taaaa\t", key="ctrl-o")  # forking it is fine
        self.assertEqual(launched["argv"][-1], "--fork-session")

    def test_fzf_gets_the_switcher_bindings_and_the_list(self):
        self.run_agents("--codex", "notes")
        args = self.fzf_calls()[-1]
        self.assertIn("ctrl-n,ctrl-o", args)
        self.assertTrue(any(a.startswith("tab:transform:") and a.endswith("--cycle next footer") for a in args))
        self.assertTrue(any(a.startswith("btab:transform:") and a.endswith("--cycle back footer") for a in args))
        self.assertTrue(any(a.startswith("right:transform:") and a.endswith("--cycle next footer") for a in args))
        self.assertTrue(any(a.startswith("left:transform:") and a.endswith("--cycle back footer") for a in args))
        self.assertEqual(args[args.index("--query") + 1], "notes")
        bar = args[args.index("--header") + 1]
        self.assertIn("\033[1;7m Codex ", bar)
        self.assertIn("←/→ 切换列表", args[args.index("--footer") + 1])

    def test_this_folder_through_main(self):
        env = {"PWD": self.game}
        res = subprocess.run([sys.executable, "-I", SCRIPT, ".", "--all"], capture_output=True, text=True, cwd=self.game,
                             env={**os.environ, **self.env, "AGENTS_DRY_RUN": "1", "PATH": self.bin + os.pathsep + os.environ["PATH"],
                                  "FAKE_FZF_LOG": self.log, "FAKE_FZF_PICK": "\tdddd4444\t", **env})
        self.assertEqual(res.stdout.strip(), "")  # dddd4444 is in work, not under game: not on the list
        res = subprocess.run([sys.executable, "-I", SCRIPT, ".", "--all"], capture_output=True, text=True, cwd=self.game,
                             env={**os.environ, **self.env, "AGENTS_DRY_RUN": "1", "PATH": self.bin + os.pathsep + os.environ["PATH"],
                                  "FAKE_FZF_LOG": self.log, "FAKE_FZF_PICK": "\teeee5555\t"})
        self.assertEqual(json.loads(res.stdout)["cwd"], self.sub)

    def test_new_from_the_command_line(self):
        _, launched = self.run_agents("new", "codex", self.game, "--", "--model", "x")
        self.assertEqual(launched, {"cwd": self.game, "argv": ["codex", "--model", "x"]})
        _, launched = self.run_agents("new", "pi", "../game")
        self.assertEqual(launched, {"cwd": self.game, "argv": ["pi"]})
        res, launched = self.run_agents("new", "claude", os.path.join(self.tmp.name, "nope"))
        self.assertIsNone(launched)
        self.assertNotEqual(res.returncode, 0)

    def test_an_empty_list_prints_nothing(self):
        shutil.rmtree(self.pi)
        res = subprocess.run([sys.executable, "-I", SCRIPT, "--list", "pi"], capture_output=True, text=True,
                             env={**os.environ, **self.env, "PATH": "/nonexistent"})
        self.assertEqual(res.stdout, "")

    def test_update_fails_loudly(self):
        curl = os.path.join(self.bin, "curl")
        with open(curl, "w") as f:
            f.write("#!/bin/sh\nexit 22\n")
        os.chmod(curl, 0o755)
        res, _ = self.run_agents("update")
        self.assertNotEqual(res.returncode, 0)
        self.assertIn("更新失败", res.stderr)

    def test_reopens_the_last_list(self):
        os.makedirs(os.path.join(self.env["XDG_STATE_HOME"], "agents"))
        with open(os.path.join(self.env["XDG_STATE_HOME"], "agents", "mode"), "w") as f:
            f.write("codex")
        self.run_agents()
        args = self.fzf_calls()[-1]
        self.assertIn("\033[1;7m Codex ", args[args.index("--header") + 1])
        with open(os.path.join(self.env["XDG_STATE_HOME"], "agents", "mode"), "wb") as f:
            f.write(b"\xff")  # a damaged state file falls back to the mixed list
        self.run_agents()
        args = self.fzf_calls()[-1]
        self.assertIn("\033[1;7m 全部 ", args[args.index("--header") + 1])

    def test_the_agent_really_starts_in_the_conversations_folder(self):
        claude = os.path.join(self.bin, "claude")
        with open(claude, "w") as f:
            f.write('#!/bin/sh\necho "pwd=$(pwd) args=$*"\n')
        os.chmod(claude, 0o755)
        env = {**os.environ, **self.env, "PATH": self.bin + os.pathsep + os.environ.get("PATH", ""),
               "FAKE_FZF_LOG": self.log, "FAKE_FZF_PICK": "\tbbbb\t", "FAKE_FZF_KEY": ""}
        res = subprocess.run([sys.executable, "-I", SCRIPT, "--all"], capture_output=True, text=True, env=env, cwd=self.work)
        self.assertEqual(res.stdout.strip(), f"pwd={self.game} args=--resume bbbb")

    def test_new_with_a_wrong_folder_or_argument(self):
        res, launched = self.run_agents("new", "cladue")
        self.assertIsNone(launched)
        self.assertIn("claude、codex、pi", res.stderr)
        res, launched = self.run_agents("new", self.game, "codex")
        self.assertIsNone(launched)
        self.assertIn("用法", res.stderr)

    def test_a_folder_it_cannot_enter(self):
        if os.geteuid() == 0:
            self.skipTest("root enters any folder")
        locked = os.path.join(self.tmp.name, "locked")
        os.makedirs(locked)
        os.chmod(locked, 0)
        try:
            env = {**os.environ, **self.env, "PATH": self.bin + os.pathsep + os.environ.get("PATH", "")}
            res = subprocess.run([sys.executable, "-I", SCRIPT, "new", "claude", locked], capture_output=True, text=True, env=env, cwd=self.work)
        finally:
            os.chmod(locked, 0o755)
        self.assertNotIn("Traceback", res.stderr)
        self.assertNotEqual(res.returncode, 0)

    def test_version(self):
        res, _ = self.run_agents("--version")
        self.assertTrue(res.stdout.startswith("agents "))


if __name__ == "__main__":
    unittest.main()
