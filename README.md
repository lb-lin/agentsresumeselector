# agents

[![test](https://github.com/lb-lin/agentsresumeselector/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/lb-lin/agentsresumeselector/actions/workflows/test.yml)
[![release](https://img.shields.io/github/v/release/lb-lin/agentsresumeselector)](https://github.com/lb-lin/agentsresumeselector/releases/latest)
[![license](https://img.shields.io/github/license/lb-lin/agentsresumeselector)](LICENSE)

在任意目录里，用一个列表找到 Claude Code、Codex 和 Pi 的对话，选中就在它原来的目录里接着聊。

```
>
  8/8 ──────────────────────────────────────────────────────────────
 [ 全部 ]  Claude   Codex   Pi
  ＋ 新建 Claude 对话…
  ＋ 新建 Codex 对话…
  ＋ 新建 Pi 对话…
    2分钟前  [Claude] ~/code/shop-api            修复登录接口 500  · 登录接口偶尔返回 500，帮我查一下
  ▌ 50分钟前 [Codex]  ~/code/blog                把首页改成按年份归档
    5小时前  [Pi]     ~/code/pinball             调一下挡板的手感
    1天前    [Claude] ~/code/notes               整理会议记录  · 整理这周的会议记录
    2天前    [Codex]  ~/code/shop-api            给订单列表加分页
────────────────────────────────────────────────────────────────────
 ←/→/Tab 切换列表 · Enter 恢复 · Ctrl-O 分支 · Ctrl-N 同目录新建 · Ctrl-X 归档（Codex）· Ctrl-D 删除 · Ctrl-/ 预览 · ● 正被占用
╭──────────────────────────────────────────────────────────────────╮
│ Codex · ~/code/blog · 更新于 50分钟前 · b2                       │
│                                                                  │
│ ── 你                                                            │
│ 把首页改成按年份归档                                             │
│                                                                  │
│ ── Codex                                                         │
│ 已按年份分组，并保留了原来的分页。                               │
╰──────────────────────────────────────────────────────────────────╯
```

## 为什么要有它

`claude -r` 默认只列当前目录的对话，要接着聊别的目录的对话，得先切到「全部」，再复制一条带 `cd` 的命令；`codex resume`、`pi --resume` 也各管各的。

`agents` 把它们放在一个列表里：

- **所有目录、所有 agent**：可以混在一起看，也可以按 agent 切换。
- **选中就能接着聊**：直接在对话原来的目录里启动 agent，退出后你还在原来的位置。
- **能搜索**：标题、目录和第一句话都能搜到。
- **能分支、能新建**：从旧对话复制一份接着聊，或者在同一个目录新开一个。
- **能删除、能归档**：确定不要的对话按 Ctrl-D 删掉；Codex 的对话还能按 Ctrl-X 用它自带的归档收起来，随时能放回。
- **知道谁正被占用**：正在别处运行的对话会标上 ●，避免两个进程写同一份记录。

## 安装

```sh
curl -fsSL https://raw.githubusercontent.com/lb-lin/agentsresumeselector/main/install.sh | sh
```

默认装到 `~/.local/bin`，不需要 root，也不需要 Python 或 Rust：`agents` 是单个可执行文件，下载时会核对校验和。没有 fzf（或版本低于 0.65）时，安装脚本会从 fzf 的 GitHub 发布页下载对应平台的版本，同样核对官方校验和。

| 系统 | 现成的程序 |
|---|---|
| Linux（静态链接） | x86_64、arm64、armv7、armv6、ppc64le |
| macOS | x86_64、arm64 |

其他平台（比如 riscv64、loongarch64、FreeBSD）上，只要装了 Rust 1.82 或更新版本，安装脚本会用 `cargo` 现场编译。

以后更新：`agents update`，会装回它现在所在的目录。

安装脚本可以用这些环境变量调整：

| 变量 | 作用 |
|---|---|
| `AGENTS_BIN_DIR` | 装到哪个目录（默认 `~/.local/bin`） |
| `AGENTS_REF` | 装哪个版本，比如 `v0.9.0`（默认最新版） |
| `AGENTS_DOWNLOAD_URL` | 不从 GitHub Release 下载，改从这个地址取程序和 `SHA256SUMS` |
| `AGENTS_SKIP_FZF` | 设为 `1` 时不检查、不安装 fzf |

## 用法

| 命令 | 作用 |
|---|---|
| `agents` | 打开上次看的列表（第一次是「全部」），最新的对话在最前 |
| `agents 弹珠` | 打开时带上搜索词 |
| `agents --all` / `--claude` / `--codex` / `--pi` | 从切换器里的某个列表开始 |
| `agents --archived` | 打开「已归档」列表（Codex 归档的对话） |
| `agents .`（或 `--here`） | 只看当前目录（含子目录） |
| `agents new [claude\|codex\|pi] [目录]` | 新建对话；不给 agent 就用上次看的那个，不给目录就从用过的目录里选，或输入一个路径 |
| `agents ... -- --model opus` | `--` 后面的参数原样交给 agent |
| `agents update` | 更新到最新版 |
| `agents --version` / `--help` | 版本和帮助 |

列表里的按键：

| 按键 | 作用 |
|---|---|
| Enter | 在对话原来的目录里恢复 |
| Ctrl-O | 分支：复制出一份新的接着聊，原对话不动 |
| Ctrl-N | 用同一个 agent 在同一个目录新开一个对话 |
| Ctrl-X | 归档 Codex 的对话：从列表里收起来，不删除，可以放回 |
| Ctrl-D | 永久删除，会先问一句，输入 `y` 才删 |
| ← / → | 搜索框为空时切换列表；输入了字时移动光标 |
| Tab / Shift-Tab | 随时切换列表（Ctrl-T 同 Tab） |
| Ctrl-/ | 显示或隐藏预览（最近几轮对话） |
| Esc | 退出 |

实际启动的命令：

| | Claude Code | Codex | Pi |
|---|---|---|---|
| 恢复 | `claude --resume <id>` | `codex resume <id>` | `pi --session <id>` |
| 分支 | `claude --resume <id> --fork-session` | `codex fork <id>` | `pi --fork <id>` |
| 新建 | `claude` | `codex` | `pi` |

## 归档和删除

只有官方提供了归档的 agent 才能归档，agents 不自己造一套：

- **Ctrl-X 归档（只有 Codex）**：调用 Codex 自带的 `codex archive`，对话移到 `~/.codex/archived_sessions/`，Codex 自己的列表里也看不到了。切换器最后会多出「已归档」，在那里：
  - Ctrl-X：放回原来的列表。
  - Enter / Ctrl-O：先放回，再恢复或分支。
  - Ctrl-D：永久删除。
- **Ctrl-D 删除**：先显示是哪个对话，输入 `y` 才删，其他输入都算取消。删除后找不回来。
- Claude Code 和 Pi 没有官方的归档，在它们的对话上按 Ctrl-X 只会提示用 Ctrl-D 删除。
- 正被占用（●）的对话不能归档，也不能删除：先在那边退出。Pi 的对话，只要它的目录里有 Pi 在运行，也不让动。
- 删除时各自怎么做：

| | 删除 |
|---|---|
| Codex | 调用 Codex 自带的 `codex delete --force`（agents 已经问过了） |
| Pi | 删掉那个 `.jsonl`，和 Pi 自己在 `pi --resume` 里按 Ctrl+D 删除的效果一样 |
| Claude Code | 官方没有删除单个对话的命令，agents 删掉 `.jsonl` 和旁边的同名文件夹（子 agent、工具输出）。`~/.claude/file-history/` 里的文件备份留给 Claude 自己清理 |

## 列表是怎么来的

- **切换器**：输入框下面是 全部 / Claude / Codex / Pi，当前的那个反色高亮。「全部」里每一行都标着 agent。下次打开时停在上次看的列表。机器上既没装、也没有留下对话的 agent 不出现。
- **标题**依次取：你起的名字（Claude 的 `/rename`、Codex 的线程名、Pi 的 `/name`），Claude 自动生成的标题或 `claude agents` 里的会话名，你说的第一句话。以斜杠命令开头、之后得到过回复的对话，用那条命令当标题。
- **不会列出**：还没说过话的空对话、只执行过一条设置命令的对话、Claude 和 Codex 内部的子 agent。一份记录坏了只会跳过它自己。
- **●** 表示对话正被运行中的 agent 占着，这时不能恢复，但可以按 Ctrl-O 分出一份。判断方式：
  - Claude：`~/.claude/sessions` 里的进程记录。
  - Codex：Linux 上看线程写锁；macOS 上只能认出用 `codex resume <id>` 打开的。
  - Pi：新版 Pi 运行时会把进程名改成 `pi`，看不出它开着哪个对话，所以不标 ●（老版本用 `pi --session <id>` 打开的能认出）。归档和删除时更保守：只要对话的目录里有 Pi 在运行，就不让动。

## 它读什么、写什么

平时只读对话记录，不上传任何内容；只有 `agents update` 会联网。只有你在列表里按 Ctrl-X 或 Ctrl-D 时，才会移动或删除那一个对话的记录，而且只动上面那几个 agent 目录里的 `.jsonl` 文件。

| 来源 | 位置 | 可以改位置的设置 |
|---|---|---|
| Claude Code | `~/.claude/projects/*/*.jsonl`、`~/.claude/sessions/*.json` | `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/sessions/**/rollout-*.jsonl`、`~/.codex/session_index.jsonl`、`~/.codex/thread-writer-locks/` | `CODEX_HOME` |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` | `PI_CODING_AGENT_SESSION_DIR`、`PI_CODING_AGENT_DIR`、Pi 的 `settings.json` 里写成绝对路径的 `sessionDir` |
| 判断是否被占用 | 运行中进程的命令行，Linux 上还有 `/proc/locks` | |

Pi 单次运行时用的 `--session-dir` 没法知道，那些对话不会列出。

除此之外唯一会写的文件是 `~/.local/state/agents/mode`（或 `$XDG_STATE_HOME/agents/mode`），记住你上次看的是哪个列表。

## 常见问题

**提示「需要 fzf」**：重新运行安装命令，或自己安装 [fzf](https://github.com/junegunn/fzf)。`agents` 优先用装在它旁边的 fzf，其次用 `PATH` 里的。

**fzf 版本低一些能用吗**：能，只是功能会少。

| fzf 版本 | 区别 |
|---|---|
| 0.65 及以上 | 全部功能 |
| 0.46 ~ 0.64 | 能切换列表、归档、删除，按键说明挤在切换器后面 |
| 低于 0.46 | 不能切换列表，也不能归档、删除；用 `--claude` 这类参数选列表 |

**提示「这个对话正被另一个进程占着」**：先在那边退出；`claude agents` 里的后台会话要先停掉。不想退出的话，在列表里按 Ctrl-O 分出一份。归档和删除也要等它退出。

**提示「目录不在了」**：这个对话原来的目录已被移动或删除。把目录放回去，或者在其他目录新开一个对话。

**运行后 shell 的当前目录没变**：这是有意的。agent 在对话的目录里运行，退出后你回到原来的位置。

## 卸载

```sh
rm ~/.local/bin/agents          # fzf 是安装脚本装的话：rm ~/.local/bin/fzf
rm -r ~/.local/state/agents
```

## 开发

需要 Rust 1.82 或更新版本。

```sh
cargo test                    # 用虚构的 Claude、Codex、Pi 记录和一个假 fzf 跑测试
cargo build --release         # 程序在 target/release/agents
```

代码结构、不能破坏的约定和发布流程写在 [AGENTS.md](AGENTS.md)。

## 许可

[MIT](LICENSE)
