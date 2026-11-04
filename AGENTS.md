# AGENTS.md

给在这个仓库里工作的编码 agent 看的说明。面向用户的介绍见 [README.md](README.md)。

`agents` 是一个 Rust 写的命令行工具：读取 Claude Code、Codex、Pi 的对话记录，用 fzf 列出来，选中后在对话原来的目录里 `exec` 对应的 agent。

## 常用命令

```sh
cargo test --locked                # 全部测试，不碰真实的对话记录
cargo build --release              # 程序在 target/release/agents
sh -n install.sh                   # 检查安装脚本语法
sh .github/build.sh <target>       # 按发布方式构建某个平台（Linux 是 musl 静态链接）
```

改完代码至少跑 `cargo test --locked`；改了 `install.sh` 还要实际跑一遍，比如用 `AGENTS_DOWNLOAD_URL=file://...` 指向本地目录，并设 `AGENTS_BIN_DIR` 装到临时目录。

## 代码结构

| 路径 | 内容 |
|---|---|
| `src/main.rs` | 入口，只把命令行参数交给库 |
| `src/lib.rs` | 全部逻辑：读记录、判断占用、生成列表、调用 fzf、启动 agent |
| `src/wide.rs` | 生成的表：East Asian Width 为 W/F 的码点（从 Python 的 `unicodedata` 导出），`pad` 对齐用 |
| `tests/test_agents.rs` | 测试，同时调用库函数和编译出的程序 |
| `install.sh` | 安装脚本，下载程序和 fzf 并核对校验和 |
| `.github/workflows/test.yml` | 每次推送跑测试、安装脚本和各平台构建 |
| `.github/workflows/release.yml` | 推送 `v*` 标签时构建并发布 |

`Ctx` 保存所有来自环境的配置（家目录、各 agent 的目录、`PATH`、`AGENTS_HERE`）。新代码通过 `Ctx` 取这些值，不要直接读环境变量，否则测试没法用虚构的目录。

fzf 回调程序自身的内部子命令：`--list <mode>`、`--cycle [next|back] [footer|inline] [arrow]`、`--preview <agent> <path> <id> <cwd>`。

## 不能破坏的约定

**数据安全**
- 对话记录只读。唯一会写的文件是状态目录下的 `mode`。
- 除了 `agents update`，不要加任何联网行为。

**测试**
- 只用 `tests/test_agents.rs` 里的 `Home`（虚构的目录）和假 fzf。不要读写真实的 `~/.claude`、`~/.codex`、`~/.pi`。
- 不要在测试或调试中启动真实的 agent。要验证启动了什么，设 `AGENTS_DRY_RUN=1`，程序会打印 `{"cwd": ..., "argv": [...]}` 而不启动。
- 不要向别人正在用的终端发送按键。要测真实的 fzf 交互，自己开一个 pty，并同样设 `AGENTS_DRY_RUN=1`。

**列表行格式**
- 一行是六个用 Tab 分隔的字段：`action`、`agent`、`id`、`cwd`、`path`、`shown`。fzf 只显示第 6 个，`--preview` 用 `{2} {5} {3} {4}` 取其余字段，`main` 也按这个顺序解析。三处要一起改。
- 进入字段的值不能含 Tab 或换行：路径和 id 过 `text_field` / `plain_field`，标题和第一句话过 `clean`。

**fzf**
- 版本分支由 `FZF_SWITCH`（0.46）、`FZF_WITH_SHELL`（0.51）、`FZF_FOOTER`（0.65）控制，低版本要继续能用。`install.sh` 的 `FZF_MIN` 要和最高的那个一致。
- `cycle` 输出里 `change-header:` 必须放在最后，它会吃掉这一行剩下的内容。
- ←/→ 是否切换列表，在 fzf 0.51 及以上由 `sh` 判断 `$FZF_QUERY`，这样在搜索框里移动光标不用启动本程序。切换列表和预览每次都会启动本程序，启动要保持在几毫秒内。

**健壮性**
- 记录文件可能损坏、截断或正在被写。一份坏记录只能跳过它自己，绝不能让整个列表失败。
- 发布版是 `panic = "abort"`，处理外部数据时不要用 `unwrap` / `expect`，用 `Option` 和 `?`。
- 输出用 `say` / `note`，不要用 `println!` / `eprintln!`：fzf 可能先关掉管道，那些宏遇到写入失败会 panic。
- `parse` 会把孤立的代理对转义（JavaScript 截断 emoji 时写出的 `\ud83d`）换成 `?`，否则整行都读不了。
- `first_typed` 跳过 `tool_result` 行不解析、不计数，这是长对话也能快速读取的关键。

## 风格

- 面向用户的文字（提示、界面、README）用中文。代码、注释、提交信息用英文。
- 新增依赖要有充分理由。目前只有 `serde_json`、`memmap2`、`memchr`、`libc`，程序要保持小而快。
- 注释写「为什么」，不写「做了什么」。
- 不只支持 Linux：`/proc` 不存在时要有退路（看 `processes` 和 `alive`），macOS 也在 CI 里。

## 常见改动

**支持新的 agent**：改 `AGENTS`、`MODES`、`name_of`、`color_of`、`Ctx::session_root`、`said_by`、`Ctx::conversations`（加读取函数）、`Ctx::launch`（恢复、分支、新建的命令）、`Ctx::held`。然后补测试，并更新 README 的命令表和「它读什么、写什么」。

**增加发布平台**：目标列表写在四处，要一起改：`install.sh` 的 `case`、`test.yml`、`release.yml`、README 的平台表。先确认 `sh .github/build.sh <target>` 能过：有些目标（riscv64、loongarch64）的 Rust 工具链不自带 musl，链接会失败。

## 发布

下载地址的约定：Release 里每个平台一个 `agents-<target>`，外加一个 `SHA256SUMS`。`release.yml` 产出，`install.sh` 读取。

1. 改 `Cargo.toml` 的 `version`，运行 `cargo build` 更新 `Cargo.lock`，提交。
2. 打标签 `vX.Y.Z` 并推送，`release.yml` 会构建并发布。带连字符的标签（`v1.0.0-rc1`）是预发布版，不会成为「最新版」，可以用 `AGENTS_REF` 指定安装。
3. 等 Release 发布后，实际跑一遍安装脚本确认。

**顺序很重要**：`agents update` 和安装命令用的是 `main` 上的 `install.sh`，下载的是最新 Release。如果 `install.sh` 依赖新的下载内容，必须先发布、再合并到 `main`，否则别人会装失败。另外 `raw.githubusercontent.com` 有几分钟缓存，刚推送后要验证时，用带提交 SHA 的地址。

## Git

- 大改动先开分支，等维护者确认后再合并到 `main`。
- 不要强推 `main` 和已发布的标签。
- 提交信息：英文，一行说清做了什么，正文解释为什么。
