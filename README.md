# TrafficMonitorBar

[TrafficMonitor](https://github.com/zhongyang219/TrafficMonitor) 的任务栏窗口，用 Rust + GPUI 重写。

- 任务栏窗口嵌入系统任务栏（Windows 10 经典任务栏 / Windows 11 任务栏），只用 Direct2D 绘制：默认走 `UpdateLayeredWindowIndirect`，失败时改走 DirectComposition。
- 选项设置、显示设置、鼠标提示、历史流量、连接详情、关于、检查更新、插件管理都是 GPUI 窗口（[gpui-component](https://github.com/longbridge/gpui-component)）。
- 通知区图标和右键菜单是 Win32 原生实现，菜单跟随系统深浅色。
- 没有主悬浮窗，没有皮肤，没有 GDI 绘制，不能更换通知区图标。

## 构建

需要 MSVC 工具链（Visual Studio 生成工具，含 Windows SDK）和 Rust stable（`x86_64-pc-windows-msvc`）。

```powershell
cargo build --release
```

`build.rs` 会同时用系统自带的 .NET Framework `csc.exe` 编译温度监控辅助程序 `TrafficMonitorBar.Hardware.exe`，输出到 `target\<profile>\`。

## 温度监控

温度监控与原版一样依赖 LibreHardwareMonitor，需要管理员权限。把原仓库的
`OpenHardwareMonitorApi\LibreHardwareMonitorLib.dll` 复制到 `TrafficMonitorBar.exe` 所在目录，再在“选项设置 > 常规设置 > 硬件监控”中勾选要监控的硬件。

未以管理员身份运行时，温度相关的项目和选项后面会显示红色的 `!`，任务栏上对应的数值也显示 `!`；可以在右键菜单中选择“以管理员身份重新启动”。

## 插件

兼容原版插件（放在程序目录的 `plugins` 文件夹）。插件项目只显示插件提供的标签和数值文本，不调用插件自绘（`DrawItem` / `DrawItemEx`）；插件的鼠标、键盘事件、命令和选项对话框照常转发。

## 数据和配置

默认保存在 `%AppData%\TrafficMonitorBar`（`config.ini`、`history_traffic.dat`）。在程序目录放置 `global_cfg.ini` 并设置 `portable_mode = true` 可改为保存到程序目录。

## 图标

- 运行时图标（通知区、窗口）：原版“图标 1”，深色任务栏用白色版本，浅色任务栏用黑色版本，自动切换。
- exe 文件图标：原版“图标 2”。
