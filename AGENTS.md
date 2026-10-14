# TrafficMonitorBar

改完会进入可执行文件的代码后，收尾前必须编译 release 并重新启动。只改文档、或本轮没有改代码时，不要编译、不要重启。

二进制是 `target\release\TrafficMonitorBar.exe`。它是单实例（互斥体 `TrafficMonitorBar_SingleInstance`）：旧进程还在时再启动会直接退出。Release 无控制台（`windows_subsystem = "windows"`）。硬件监控库 `TrafficMonitorBar.Hardware.dll` 由主进程加载。目录里若还留着旧的 `TrafficMonitorBar.Hardware.exe`，构建脚本会结束该进程并删除它。

在仓库根目录执行 `.\build.ps1`。脚本会先结束本仓库 `target\release` 里的这两个进程，再执行 `cargo build --release`。编译成功后再启动，失败则不要启动。Release 编译可能要几分钟，等到脚本结束再启动：

```powershell
.\build.ps1
if (-not $?) { exit 1 }
Start-Process -FilePath ".\target\release\TrafficMonitorBar.exe" -WorkingDirectory ".\target\release"
```
