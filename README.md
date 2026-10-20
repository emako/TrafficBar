# TrafficBar

[TrafficMonitor](https://github.com/zhongyang219/TrafficMonitor) 的任务栏窗口，用 Rust + GPUI 重写。

- 任务栏窗口嵌入系统任务栏（Windows 10 经典任务栏 / Windows 11 任务栏），只用 Direct2D 绘制：默认走 `UpdateLayeredWindowIndirect`，失败时改走 DirectComposition。
- 选项设置、显示设置、鼠标提示、历史流量、连接详情、关于、检查更新、插件管理都是 GPUI 窗口（[gpui-component](https://github.com/longbridge/gpui-component)）。
- 通知区图标和右键菜单是 Win32 原生实现，菜单跟随系统深浅色。
- 没有主悬浮窗，没有皮肤，没有 GDI 绘制，不能更换通知区图标。

## 构建

需要 MSVC 工具链（Visual Studio 生成工具，含 Windows SDK）、Rust stable（`x86_64-pc-windows-msvc`），以及系统自带的 .NET Framework 4（`csc.exe`）。

```powershell
cargo build --release
```

`build.rs` 会用 `dotnet` 编译硬件监控库，再把 `TrafficBar.Hardware.dll` 嵌进 `TrafficBar.exe`。主程序从内存加载它，运行目录里没有这份 DLL。LibreHardwareMonitor 已嵌在该库内。

## 安装包

需要 [.NET SDK 8+](https://dotnet.microsoft.com/download)（用于全局工具 `vpk`）。在仓库根目录执行：

```powershell
.\pack.ps1
```

脚本会先 release 编译，再用 [Velopack](https://docs.velopack.io/) 把 `TrafficBar.exe` 打成安装包，输出到 `target\setup\`：

安装包文件名里的版本取自 `Cargo.toml`。当前版本的三个发布文件是：

| 文件 | 用途 |
| --- | --- |
| `TrafficBar-1.0.0-x64.exe` | 一键安装（当前用户，`%LocalAppData%\TrafficBar`） |
| `TrafficBar-1.0.0-x64.msi` | MSI，当前用户安装 |
| `TrafficBar-1.0.0-x64.zip` | 可选，不经安装直接解压运行 |
| `releases.win.json`、`*-full.nupkg`（及可选 delta） | 程序内「检查更新」用的更新源 |

发布到 GitHub / Gitee Release 时请附上上述安装包与更新源文件。安装包目前未代码签名，SmartScreen 可能拦截。

`packId` 固定为 `TrafficBar`，请勿更改，否则已安装用户无法收到后续更新。

## 温度监控

温度监控与原版一样依赖 LibreHardwareMonitor，需要管理员权限。在“选项设置 > 常规设置 > 硬件监控”中勾选要监控的硬件。

未以管理员身份运行时，温度相关的项目和选项后面会显示红色的 `!`，任务栏上对应的数值也显示 `!`；可以在右键菜单中选择“以管理员身份重新启动”。

## 插件

兼容原版插件。直接运行时，dll 放在程序目录的 `plugins` 文件夹。用安装包装上之后改放到 `%AppData%\TrafficBar\plugins`：安装包重装会删掉整个 `%LocalAppData%\TrafficBar`（含 `Update.exe` 旁的文件），放在 Roaming 里才不会丢。程序内更新本来就不会动这个目录。启动时会把安装目录、`current`、`app-*` 以及安装包重装时留下的旧目录里的插件搬过来。插件项目只显示插件提供的标签和数值文本，不调用插件自绘（`DrawItem` / `DrawItemEx`）；插件的鼠标、键盘事件、命令和选项对话框照常转发。

## 数据和配置

默认保存在 `%AppData%\TrafficBar`（`config.ini`、`history_traffic.dat`）。在程序目录放置 `global_cfg.ini` 并设置 `portable_mode = true` 可改为保存到程序目录（Velopack 安装下也是安装根目录，更新不会覆盖）。

## 图标

- 运行时图标（通知区、窗口）：原版“图标 1”，深色任务栏用白色版本，浅色任务栏用黑色版本，自动切换。
- exe 文件图标：原版“图标 2”。
