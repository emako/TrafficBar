# TrafficBar

[TrafficMonitor](https://github.com/zhongyang219/TrafficMonitor) 的任务栏窗口，用 Rust + GPUI 重写， TrafficMonitor 不支持深色模式，桌面悬浮窗功能有点冗余。

- 任务栏窗口嵌入 Windows 系统任务栏，只用 Direct2D 绘制，不支持 GDI 绘制。
- 选项设置、显示设置、更改通知区图标、鼠标提示、历史流量、连接详情、关于、检查更新、插件管理都是 [GPUI](https://github.com/longbridge/gpui-component) 窗口。
- 通知区图标和右键菜单是 Win32 原生实现，菜单跟随系统深浅色。
- 界面多语言，可跟随系统或在选项里指定；语言文件在 `src/assets/lang/`。
- 没有主悬浮窗，没有皮肤，没有 GDI 绘制。

## 构建

需要 MSVC 工具链（Visual Studio 生成工具，含 Windows SDK）、Rust stable（`x86_64-pc-windows-msvc`）、[.NET SDK 10+](https://dotnet.microsoft.com/download)，以及 [.NET Framework 4.x 开发人员包](https://dotnet.microsoft.com/download/dotnet-framework)（提供 `metahost.h` / `mscoree.h`；运行时仍用系统自带的 .NET Framework 4）。

```powershell
cargo build --release
```

## 安装包

同样需要 .NET SDK 10+（用于全局工具 `vpk`）。在仓库根目录执行：

```powershell
cargo setup
```

（等价于 `.\setup.ps1`。）

脚本会先 release 编译，再用 [Velopack](https://docs.velopack.io/) 把 `TrafficBar.exe` 打成安装包，输出到 `target\publish\`：

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

温度监控依赖 LibreHardwareMonitor，需要管理员权限。在“选项设置 > 常规设置 > 硬件监控”中勾选要监控的硬件。

未以管理员身份运行时，温度相关的项目和选项后面会显示红色的 `!`，任务栏上对应的数值也显示 `!`；可以在右键菜单中选择“以管理员身份重新启动”。

## 效率模式

可在“选项设置 > 常规设置”中开启。会降低本进程优先级并启用 EcoQoS，任务管理器会显示绿叶；仅 Windows 11 生效。

## 插件

兼容原版插件。直接运行时，dll 放在程序目录的 `plugins` 文件夹。用安装包装上之后改放到 `%AppData%\TrafficBar\plugins`：安装包重装会删掉整个 `%LocalAppData%\TrafficBar`（含 `Update.exe` 旁的文件），放在 Roaming 里才不会丢。程序内更新本来就不会动这个目录。启动时会把安装目录、`current`、`app-*` 以及安装包重装时留下的旧目录里的插件搬过来。插件项目只显示插件提供的标签和数值文本，不调用插件自绘（`DrawItem` / `DrawItemEx`）；插件的鼠标、键盘事件、命令和选项对话框照常转发。

## 数据和配置

默认保存在 `%AppData%\TrafficBar`（`config.ini`、`history_traffic.dat`）。只有便携版（没有 `Update.exe` 的 zip）能改成保存到程序目录，对应程序目录下 `global_cfg.ini` 的 `portable_mode = true`。安装版会忽略这个选项，并把它改回 AppData。

