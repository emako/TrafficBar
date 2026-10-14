# TrafficMonitorBar

改完会进入可执行文件的代码后，收尾前必须编译 release 并重新启动。只改文档、或本轮没有改代码时，不要编译、不要重启。

二进制是 `target\release\TrafficMonitorBar.exe`。它是单实例（互斥体 `TrafficMonitorBar_SingleInstance`）：旧进程还在时再启动会直接退出。Release 无控制台（`windows_subsystem = "windows"`）。硬件辅助进程是同目录的 `TrafficMonitorBar.Hardware.exe`，运行时也会锁住该文件。

在仓库根目录用 PowerShell 执行，且只结束本仓库 release 输出，不要结束装在其他目录的同名程序：

```powershell
$root = (Resolve-Path .).Path
$exe = Join-Path $root "target\release\TrafficMonitorBar.exe"
$helper = Join-Path $root "target\release\TrafficMonitorBar.Hardware.exe"
Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -in @($exe, $helper) } | ForEach-Object {
    Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
}
$deadline = (Get-Date).AddSeconds(15)
do {
    $left = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -in @($exe, $helper) })
    if ($left.Count -eq 0) { break }
    Start-Sleep -Milliseconds 200
} while ((Get-Date) -lt $deadline)

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe)
```

先停进程再编译，否则链接无法覆盖正在运行的 exe。编译失败就停在这里，不要启动。`cargo build --release` 可能要几分钟，等到结束再启动。
