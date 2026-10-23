# TrafficBar

## Agent Skills

仓库级 skills 统一放在 `docs/skills/`（每个 skill 一个子目录，内含 `SKILL.md`）。需要某项工作流时先读对应 `SKILL.md` 并按其执行；新增 skill 也写到该目录，不要放到 `.cursor/skills/`。

当前 skills：

- `docs/skills/fluent-icon-providor` — 为 `src/assets/icons/menu` 从 Fluent System Icons 制作 light/dark ICO

改完会进入可执行文件的代码后，收尾前必须打到安装包并重新启动。只改文档、或本轮没有改代码时，不要打包、不要重启。

二进制默认是 `target\release\TrafficBar.exe`（宿主架构、未显式 `--target`）。交叉编译时在 `target\<triple>\release\TrafficBar.exe`。它是单实例（互斥体 `TrafficBar_SingleInstance`）：旧进程还在时再启动会直接退出。Release 无控制台（`windows_subsystem = "windows"`）。硬件监控库嵌在 exe 中，从内存加载，不再放在运行目录。目录里若还留着旧的 `TrafficBar.Hardware.exe`，构建脚本会结束该进程并删除它；残留的 `TrafficBar.Hardware.dll` 会在编译时删掉。

支持的打包 target：`x86_64-pc-windows-msvc`（产物后缀 `-x64`，Velopack channel / runtime `win-x64`，更新清单 `releases.win-x64.json`）、`aarch64-pc-windows-msvc`（后缀 `-arm64`，channel / runtime `win-arm64`，更新清单 `releases.win-arm64.json`）。WoA 出原生 ARM64；原生 ARM64 进程不能加载原版 x64 插件 DLL。

## 多语言

任何多语言修改必须同步所有语言，不能漏语言。语言文件在 `src/assets/lang/`，每种语言一个 ini，键集合不能比 `en-US.ini` 少。翻译必须由 AI 完成，禁止使用 API 服务翻译。AI 翻译之后要校对用词是否够本地化：沿用该语言里已经在用的说法，产品名、占位符（`%s`、`%d`、`<%1%>`）和快捷键标记（`&`、`(&R)`）保持原样。

在仓库根目录执行 `cargo setup`（或 `.\setup.ps1`）。它会先跑 `.\build.ps1`（结束对应 release 目录里的进程，再 `cargo build --release`），再用 Velopack 打出安装包。产物在 `target\publish`（`TrafficBar-<版本>-x64` / `-arm64` 的 `.exe`、`.msi`、`.zip`，以及仅含 exe 的 `single-*.zip`）。打包成功后再启动，失败则不要启动。打包可能要几分钟，等到脚本结束再启动：

```powershell
cargo setup
if (-not $?) { exit 1 }
Start-Process -FilePath ".\target\release\TrafficBar.exe" -WorkingDirectory ".\target\release"
```

ARM64 示例：`cargo setup -- -Target aarch64-pc-windows-msvc`，启动路径改为 `.\target\aarch64-pc-windows-msvc\release\TrafficBar.exe`（仅在 ARM 机上可运行）。
