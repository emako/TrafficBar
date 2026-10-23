---
name: fluent-icon-providor
description: >-
  为 src/assets/icons/menu 新增 Fluent System Icons 菜单 ICO（light/dark）。
  从 NuGet 缓存 fluentui.systemicons.assets 取 Regular SVG，经 D:\GitHub\svg2png
  转 PNG 后按现有尺寸顺序合成 ICO。在用户要给 menu 加图标、新增 *_light.ico /
  *_dark.ico、或提到 fluent-icon-providor / Fluent menu icon 时使用。
---

# Fluent 菜单图标（fluent-icon-providor）

给 `src/assets/icons/menu` 新增一对 `{name}_light.ico` / `{name}_dark.ico`。

## 目标规格（必须对齐现有文件）

以 `admin_dark.ico` 为准（见 `src/tray/icons.rs`）：

| 项 | 值 |
|---|---|
| 帧格式 | PNG 嵌入 ICO |
| 尺寸与顺序 | **16 → 20 → 24 → 28 → 32**（目录项按此顺序） |
| 色深 | 32-bit（RGBA） |
| light | 前景约 `#212121`（Fluent Regular 默认） |
| dark | 前景 `#FFFFFF` |
| 命名 | `{stem}_light.ico` / `{stem}_dark.ico` |

不要用 Pillow 默认 `save(..., format="ICO")` 多尺寸导出（常只写出一帧）。用本 skill 的 `scripts/pack_menu_ico.py`。

## 工作流

复制并跟踪：

```
进度：
- [ ] 1. 定位 NuGet 包最新版 Regular SVG 目录
- [ ] 2. 选定 Fluent 图标名（ic_fluent_<glyph>_*_regular.svg）
- [ ] 3. 为 light / dark 各准备 16/20/24/28/32 的着色 SVG
- [ ] 4. 用 D:\GitHub\svg2png 转成同尺寸 PNG
- [ ] 5. pack_menu_ico.py 合成两枚 ICO 到 src/assets/icons/menu
- [ ] 6. 若要进托盘菜单：在 src/tray/icons.rs 注册并接线
- [ ] 7. 校验 ICO 帧顺序与色相
```

### 1. 找 SVG 源

在本机 NuGet 缓存取 **最新版本**：

```text
%USERPROFILE%\.nuget\packages\fluentui.systemicons.assets\<version>\assets\svg\FluentSystemIcons-Regular\
```

示例（版本号会变，以目录里最大 semver 为准）：

```text
C:\Users\McCoy\.nuget\packages\fluentui.systemicons.assets\1.1.323\assets\svg\FluentSystemIcons-Regular
```

PowerShell 取最新版路径：

```powershell
$root = Join-Path $env:USERPROFILE ".nuget\packages\fluentui.systemicons.assets"
$ver = Get-ChildItem $root -Directory | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
$svgDir = Join-Path $ver.FullName "assets\svg\FluentSystemIcons-Regular"
```

文件名模式：`ic_fluent_<glyph>_<size>_regular.svg`（如 `ic_fluent_settings_24_regular.svg`）。

**缺尺寸：** 若某 size 没有对应 SVG，用最接近的可用 size 的 SVG，再用 svg2png 的 `--width/--height` 渲到目标像素（优先用更大的源，例如缺 28 时用 32）。

### 2. 着色

- **light：** 保持 / 设为 `fill="#212121"`（包内 Regular 已是该色时可直接用）。
- **dark：** 将 SVG 中前景 `fill` 改为 `#FFFFFF`（替换 `#212121` / `#000000` / `black` 等；不要动 `fill="none"`）。

每个主题、每个尺寸各一份临时 SVG。

### 3. SVG → PNG（svg2png）

工具绝对路径：`D:\GitHub\svg2png`。

优先：

```powershell
node "D:\GitHub\svg2png\dist\cli.js" <input.svg> -o <out.png> --width <N> --height <N>
```

若无 `dist`，先在该目录 `npm install` 且 `npm run build`；或：

```powershell
npx --prefix "D:\GitHub\svg2png" ts-node "D:\GitHub\svg2png\src\cli.ts" <input.svg> -o <out.png> --width <N> --height <N>
```

对每个主题跑齐 `N ∈ {16,20,24,28,32}`。需要系统已装 Microsoft Edge（Playwright 用系统 Edge）。不要依赖可能损坏的 `release\svg2png.exe` 缓存 stub。

一键流水线（推荐）：

```powershell
python .cursor/skills/fluent-icon-providor/scripts/make_menu_icon.py `
  --glyph settings `
  --stem settings `
  --svg2png "D:\GitHub\svg2png"
```

`--glyph` 为 Fluent 文件名中的 glyph 段；`--stem` 为输出文件名前缀（可与 glyph 不同）。

### 4. PNG → ICO

```powershell
python .cursor/skills/fluent-icon-providor/scripts/pack_menu_ico.py `
  --out src/assets/icons/menu/settings_light.ico `
  --png 16=path\to\16.png --png 20=... --png 24=... --png 28=... --png 32=...
```

或对某目录（内含 `16.png` … `32.png`）：

```powershell
python .cursor/skills/fluent-icon-providor/scripts/pack_menu_ico.py `
  --out src/assets/icons/menu/settings_light.ico `
  --dir path\to\light_pngs
```

### 5. 接入托盘菜单（需要显示时）

仅把文件丢进 `menu/` 不会自动出现在菜单。还需改 `src/tray/icons.rs`：

1. `include_bytes!` 增加 light/dark
2. `COUNT` / `Set` 字段 / `from_bits` / `ico()` match 同步扩展
3. 调用方挂上对应 `HBITMAP`

### 6. 校验

```powershell
python .cursor/skills/fluent-icon-providor/scripts/pack_menu_ico.py --verify src/assets/icons/menu/<stem>_light.ico
python .cursor/skills/fluent-icon-providor/scripts/pack_menu_ico.py --verify src/assets/icons/menu/<stem>_dark.ico
```

期望：5 帧、顺序 16/20/24/28/32、均为 PNG、32bpp。

## 约定

- 只使用 **FluentSystemIcons-Regular**，不用 Filled。
- 成对交付 light + dark，不要只做一边。
- 占位符/产品名无关；图标语义选最贴切的 Fluent glyph，不确定时先列候选再让用户选。
- 改完会进 exe 的资源后，按仓库 `AGENTS.md`：`cargo setup` 再启动（本 skill 本身不替代该收尾）。
