# TrafficBar

改完会进入可执行文件的代码后，收尾前必须打到安装包并重新启动。只改文档、或本轮没有改代码时，不要打包、不要重启。

二进制是 `target\release\TrafficBar.exe`。它是单实例（互斥体 `TrafficBar_SingleInstance`）：旧进程还在时再启动会直接退出。Release 无控制台（`windows_subsystem = "windows"`）。硬件监控库嵌在 exe 中，从内存加载，不再放在运行目录。目录里若还留着旧的 `TrafficBar.Hardware.exe`，构建脚本会结束该进程并删除它；残留的 `TrafficBar.Hardware.dll` 会在编译时删掉。

## 多语言

任何多语言修改必须同步所有语言，不能漏语言。语言文件在 `src/assets/lang/`，每种语言一个 ini，键集合不能比 `en-US.ini` 少。翻译必须由 AI 完成，禁止使用 API 服务翻译。AI 翻译之后要校对用词是否够本地化：沿用该语言里已经在用的说法，产品名、占位符（`%s`、`%d`、`<%1%>`）和快捷键标记（`&`、`(&R)`）保持原样。

在仓库根目录执行 `cargo setup`（或 `.\setup.ps1`）。它会先跑 `.\build.ps1`（结束本仓库 `target\release` 里的这两个进程，再 `cargo build --release`），再用 Velopack 打出安装包。产物在 `target\publish`（`TrafficBar-<版本>-x64.exe`、同名 `.msi` 和 `.zip`）。打包成功后再启动，失败则不要启动。打包可能要几分钟，等到脚本结束再启动：

```powershell
cargo setup
if (-not $?) { exit 1 }
Start-Process -FilePath ".\target\release\TrafficBar.exe" -WorkingDirectory ".\target\release"
```
