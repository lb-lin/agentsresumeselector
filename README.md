# agents

在任意目录里，用一个列表找到 Claude Code、Codex 和 Pi 的对话，选中就在它原来的目录里接着聊。

`claude -r` 只列当前目录的对话，换目录要先切到「全部」，再复制一条带 `cd` 的命令；`codex resume`、`pi --resume` 也各管各的。`agents` 列出所有目录的对话，可以按 agent 切换着看，也可以混在一起看；选中就直接在那个目录里启动，退出后回到你原来的位置。

```
>
 [ 全部 ]  Claude   Codex   Pi
  ＋ 新建 Claude 对话…
  ＋ 新建 Codex 对话…
● 1分钟前  [Claude] ~                    Orca 与 T3 Code 工具对比
  5分钟前  [Claude] ~/livegame           互动游戏
  1天前    [Codex]  ~/dshlive            继续完成打工经济玩法任务
  1天前    [Claude] ~/livegame           更合适的弹珠游戏 · 我们现在有更合适的弹珠游戏吗
 Tab 切换列表 · Enter 恢复 · Ctrl-O 分支 · Ctrl-N 同目录新建 · Ctrl-/ 预览 · ● 正被占用
```

输入框下面是切换器：全部、Claude、Codex、Pi，当前的那个用它的颜色高亮。按 **Tab** 切到下一个，**Shift-Tab** 切回上一个（Ctrl-T 也行）。「全部」里每一行都标着是哪个 agent。下次打开时停在你上次看的那个列表。机器上既没装、也没有留下对话的 agent 不出现在切换器里。

## 安装

```sh
curl -fsSL https://raw.githubusercontent.com/lb-lin/agentsresumeselector/main/install.sh | sh
```

装到 `~/.local/bin`（`AGENTS_BIN_DIR` 可改），不需要 root。需要 Python 3.8+；没有 fzf（或版本低于 0.65）时，会从 fzf 的 GitHub 发布页下载对应平台的版本，并核对官方校验和。Linux 和 macOS 都行，常见的 x86_64、arm64、armv7 等架构都支持。以后更新：`agents update`，会装回它现在所在的目录。

## 用法

| 命令 | 作用 |
|---|---|
| `agents` | 打开上次看的列表（第一次是「全部」），最新的对话在最前 |
| `agents 弹珠` | 打开时带上搜索词 |
| `agents --all` / `--claude` / `--codex` / `--pi` | 从切换器里的某个列表开始 |
| `agents .` | 只看当前目录（含子目录） |
| `agents new [claude\|codex\|pi] [目录]` | 新建对话；不给目录就从用过的目录里选，或输入一个路径 |
| `agents ... -- --model opus` | `--` 后面的参数原样交给 agent |
| `agents update` | 更新到最新版 |

列表里：

| 按键 | 作用 |
|---|---|
| Enter | 在对话原来的目录里恢复（`claude --resume <id>` / `codex resume <id>` / `pi --session <id>`） |
| Ctrl-O | 分支：从这个对话复制出一份新的接着聊，原对话不动（`claude --resume <id> --fork-session` / `codex fork <id>` / `pi --fork <id>`） |
| Ctrl-N | 用同一个 agent 在同一个目录新开一个对话 |
| Tab / Shift-Tab | 在 全部 / Claude / Codex / Pi 之间切换（Ctrl-T 同 Tab） |
| Ctrl-/ | 显示或隐藏预览（最近几轮对话） |

- 标题依次取：你起的名字（Claude 的 `/rename`、Codex 的线程名、Pi 的 `/name`），Claude 自动生成的标题或 `claude agents` 里的会话名，你说的第一句话。第一句话的前 300 个字也能被搜到；以斜杠命令开头、之后得到过回复的对话，用那条命令当标题。
- 还没说过话的空对话、只执行过一条设置命令的对话、Claude 和 Codex 内部的子 agent 不会列出。一份记录坏了只会跳过它自己，不影响列表。
- **●** 表示这个对话正被一个运行中的 agent 占着，这时不能恢复，但可以按 Ctrl-O 分出一份。Claude 看 `~/.claude/sessions` 里的进程记录；Codex 在 Linux 上看它的线程写锁，macOS 上只能认出用 `codex resume <id>` 打开的；Pi 只能认出用 `pi --session <id>` 打开的。

## 它读什么

只读对话记录，不改它们：

- Claude Code：`~/.claude/projects/*/*.jsonl`（或 `$CLAUDE_CONFIG_DIR`），`~/.claude/sessions/*.json`
- Codex：`~/.codex/sessions/**/rollout-*.jsonl`、`~/.codex/session_index.jsonl`（或 `$CODEX_HOME`），`~/.codex/thread-writer-locks/`
- Pi：`~/.pi/agent/sessions/**/*.jsonl`（或 `$PI_CODING_AGENT_SESSION_DIR`、`$PI_CODING_AGENT_DIR`，以及 Pi 的 `settings.json` 里写成绝对路径的 `sessionDir`；单次运行时用的 `--session-dir` 没法知道）
- 判断「正被占用」时：正在运行的进程的命令行，Linux 上还有 `/proc/locks`

唯一会写的：`~/.local/state/agents/mode`（或 `$XDG_STATE_HOME/agents/mode`），记住你上次看的是哪个列表。

## 卸载

```sh
rm ~/.local/bin/agents          # fzf 是安装脚本装的话：rm ~/.local/bin/fzf
rm -r ~/.local/state/agents
```

## 开发

```sh
python3 -m unittest discover -s tests
```

MIT License
