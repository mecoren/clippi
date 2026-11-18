#requires -Version 5.1
<#
.SYNOPSIS
    采集 AC-11 视觉基准：三窗口 × 浅/暗 × 当前显示器 DPI。

.DESCRIPTION
    在独立便携目录里跑一份 clippi（不读写 %APPDATA%\cn.wait.clippi.dev，
    不碰用户历史库），用自造测试数据填列表，然后屏幕抓取三个窗口。

    PrintWindow 对 GPUI 的 D3D11 窗口返回全黑，因此必须把窗口置于前台后
    用 CopyFromScreen；DwmGetWindowAttribute(EXTENDED_FRAME_BOUNDS) 提供
    可见边界，避免把 DWM 阴影算进去。

    125% / 150% 无法由脚本切换（缩放是显示器级系统设置）：先在 Windows
    设置里改缩放，再带 -ExpectDpi 120 / 144 运行，脚本会校验实际 DPI 与
    目标一致后才落盘。

.PARAMETER Theme
    light 或 dark。写进便携 clippi.toml，重启后生效。

.PARAMETER ExpectDpi
    期望的窗口 DPI（96/120/144）。0 = 不校验，按实际值归档。

.EXAMPLE
    powershell -File scripts/capture_ui_baseline.ps1 -Theme dark -ExpectDpi 96
#>
param(
    [ValidateSet('light', 'dark')]
    [string]$Theme = 'dark',
    [string]$OutRoot = '',
    [int]$ExpectDpi = 0,
    [switch]$SkipBuild,
    # 侧栏窗口背景透明，截图会把背后的应用内容一起抓进来。开启后会先把其他
    # 顶层窗口最小化几秒再恢复 —— 会打断正在使用电脑的人，默认关闭。
    [switch]$IsolateSidebar
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Drawing

$repo = Split-Path -Parent $PSScriptRoot
if (-not $OutRoot) { $OutRoot = Join-Path $repo 'docs/ui-redesign/baseline' }
$exe = Join-Path $repo 'target/debug/clippi.exe'
$runDir = Join-Path $repo 'target/ui-baseline'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class UiBaseline {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern int GetWindowLongW(IntPtr h, int index);

    /// 侧栏窗口背景透明，屏幕抓取会把它背后的应用内容一起抓进来；
    /// 截图前把其他顶层窗口最小化，让背景只剩桌面壁纸，基准才可复现。
    public static IntPtr[] Others(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid || !IsWindowVisible(h)) return true;
            var t = new StringBuilder(8); GetWindowTextW(h, t, 8);
            if (t.Length == 0) return true;
            var c = new StringBuilder(64); GetClassNameW(h, c, 64);
            string cls = c.ToString();
            if (cls == "Progman" || cls == "Shell_TrayWnd" || cls == "WorkerW") return true;
            if ((GetWindowLongW(h, -20) & 0x80) != 0) return true;
            list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list.ToArray();
    }

    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static void AltKey(byte key) {
        keybd_event(0x12, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(25);
        keybd_event(key, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(35);
        keybd_event(key, 0, 2, UIntPtr.Zero); System.Threading.Thread.Sleep(25);
        keybd_event(0x12, 0, 2, UIntPtr.Zero);
    }

    public static void Escape() {
        keybd_event(0x1B, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(30);
        keybd_event(0x1B, 0, 2, UIntPtr.Zero);
    }

    public static string Title(IntPtr h) { var t = new StringBuilder(512); GetWindowTextW(h, t, 512); return t.ToString(); }

    public static IntPtr[] OfProcess(uint target) {
        var list = new List<IntPtr>();
        EnumWindows((h, l) => { uint pid; GetWindowThreadProcessId(h, out pid); if (pid == target) list.Add(h); return true; }, IntPtr.Zero);
        return list.ToArray();
    }

    public static IntPtr FindMain(uint target, string exactTitle) {
        foreach (var h in OfProcess(target)) if (Title(h) == exactTitle) return h;
        return IntPtr.Zero;
    }

    public static IntPtr FindContains(uint target, string part) {
        foreach (var h in OfProcess(target)) if (Title(h).Contains(part)) return h;
        return IntPtr.Zero;
    }

    /// Narrow visible Zed::Window with an empty title: the tag sidebar strip.
    public static IntPtr FindSidebar(uint target, IntPtr exclude) {
        foreach (var h in OfProcess(target)) {
            if (h == exclude || !IsWindowVisible(h) || Title(h).Length != 0) continue;
            var cls = new StringBuilder(64); GetClassNameW(h, cls, 64);
            if (cls.ToString() != "Zed::Window") continue;
            RECT r; GetWindowRect(h, out r);
            int w = r.R - r.L;
            if (w > 0 && w < 120) return h;
        }
        return IntPtr.Zero;
    }

    public static RECT Frame(IntPtr h) {
        RECT f;
        if (DwmGetWindowAttribute(h, 9, out f, Marshal.SizeOf(typeof(RECT))) != 0) GetWindowRect(h, out f);
        return f;
    }

    public static string Describe(IntPtr h) {
        var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        return string.Format("hwnd=0x{0:X} vis={1} class={2} title=[{3}]", h.ToInt64(), IsWindowVisible(h), c, t);
    }

    public static bool WaitVisible(IntPtr h, int ms) {
        var sw = System.Diagnostics.Stopwatch.StartNew();
        while (sw.ElapsedMilliseconds < ms) {
            if (h != IntPtr.Zero && IsWindowVisible(h)) return true;
            System.Threading.Thread.Sleep(60);
        }
        return h != IntPtr.Zero && IsWindowVisible(h);
    }
}
'@

[UiBaseline]::SetProcessDPIAware() | Out-Null
$screenW = [UiBaseline]::GetSystemMetrics(0)
$screenH = [UiBaseline]::GetSystemMetrics(1)

function Write-Note([string]$text) { Write-Host "  $text" }

# ---------------------------------------------------------------- preflight
if (Get-Process clippi -ErrorAction SilentlyContinue) {
    $running = Get-Process clippi | ForEach-Object { "$($_.Id) $($_.Path)" }
    throw "clippi 已在运行（dev 单实例端口 19877 会被占用）。先退出正在运行的实例：`n$($running -join "`n")"
}
if (-not (Test-Path $exe)) { throw "找不到 $exe，先构建。" }
if (-not $SkipBuild) {
    Write-Host '[1/6] cargo build'
    Push-Location $repo
    try { & cargo build 2>&1 | Select-Object -Last 3 | ForEach-Object { Write-Note $_ } }
    finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw 'cargo build 失败' }
}

# ---------------------------------------------------------------- portable run dir
Write-Host '[2/6] 准备便携运行目录'
if (Test-Path $runDir) { Remove-Item $runDir -Recurse -Force }
New-Item -ItemType Directory -Path $runDir -Force | Out-Null
Copy-Item $exe (Join-Path $runDir 'clippi.exe')

# 0 字节 clippi.db 同时做两件事：让 SQLite 视作空库并建 schema；
# 阻止 `paths::migrate_portable_data` 把用户 %APPDATA% 库里真实剪贴板
# 内容复制进来（它只在 exe 目录没有 clippi.db 时迁移）。
New-Item -ItemType File -Path (Join-Path $runDir 'clippi.db') | Out-Null

# 必须是完整字段集：属性里缺 `type_filter_config` 等无 default 的字段时
# toml 会报 "line 1, column 1"，应用随后回退默认并覆盖本文件。
$config = @"
theme = "$Theme"
theme_color = "default"
hotkey = "Alt+V"
replace_system_win_v = false
quick_hotkey = "Alt+C"
quick_hotkey_enabled = true
quick_window_sync_filters = true
paste_plain_hotkey = ""
auto_start = false
auto_hide = false
db_path = ""
sort_by_created = false
search_favorites_first = false
window_position_mode = "center"
saved_window_x = -1
saved_window_y = -1
card_height_mode = "auto"
font_size_level = "standard"
font_family = ""
silent_start = true
show_source_app = false
auto_scroll_to_top = false
copy_as_plain_text = false
show_original_on_hover = false
saved_window_width = 0.0
saved_window_height = 0.0
window_geometry_version = 1
sync_enabled = false
sync_data_dir = ""
sync_device_name = ""
sync_last_at = ""
sync_interval_secs = 60
sync_backends = []
sync_auto_enabled = false
saved_enabled_backend_ids = []
sync_favorites_only = true
sync_include_images = false
sync_compress_images = false
transfer_station_enabled = false
emoji_panel_enabled = false
source_app_filter_enabled = false
reset_source_filter_on_show = false
recent_emojis = []
recent_kaomojis = []
recent_symbols = []
transfer_backend_id = ""
transfer_retention_days = 3
max_items = 0
retention_days = 0
hotkey_blacklist = []
clipboard_app_blacklist = []
record_images = true
record_files = true
privacy_protection_types = ["secret", "phone", "email", "link"]
language = "zh_CN"
pinned_tag_ids = [1, 2, 3]
tag_order = []
ocr_enabled = false
auto_detect_links = false
delete_source_after_link_extraction = false
qr_enabled = true
hide_taskbar_icon = false
block_system_window_behaviors = false
auto_focus_search = false
clear_search_on_show = false
confirm_delete = true
paste_shortcuts = []
auto_check_updates = false
update_last_check_at = ""
update_channel = "auto"
auto_fetch_url_title = false
copy_sound_enabled = false
copy_sound_file = "copy_penclick.wav"
filter_foreign_paths = false
cleanup_interval = "daily"
cleanup_stale_items = false
cleanup_last_date = ""
retention_cleanup_last_date = ""
transfer_cleanup_last_date = ""
always_reset_to_clipboard = false
image_alt_mode = "bitmap"
paste_click_mode = "double_click"
edit_auto_save = false
alt_shortcuts_enabled = true
latest_hotkeys = []

[[type_filter_config]]
key = "plain_text"
visible = true

[[type_filter_config]]
key = "rich_text"
visible = true

[[type_filter_config]]
key = "image"
visible = true

[[type_filter_config]]
key = "file"
visible = true

[[type_filter_config]]
key = "link"
visible = true

[[type_filter_config]]
key = "path"
visible = true

[[type_filter_config]]
key = "color"
visible = true

[[type_filter_config]]
key = "contact"
visible = true
"@
[System.IO.File]::WriteAllText((Join-Path $runDir 'clippi.toml'), $config, (New-Object System.Text.UTF8Encoding($false)))
Write-Note "theme=$Theme  dir=$runDir"

# ---------------------------------------------------------------- clipboard seed
$savedClip = $null
try { $savedClip = Get-Clipboard -Raw -ErrorAction SilentlyContinue } catch { }
Write-Note '剪贴板文本将在结束时恢复；图片/文件剪贴板无法恢复'

# 全部为自造数据，且不含 URL：链接标题靠网络抓取，离线/超时会让基准图不稳定。
$seedTexts = @(
    'Clippi UI baseline · plain text sample 1',
    'The quick brown fox jumps over the lazy dog. 0123456789',
    "fn main() {`n    println!(""baseline"");`n}",
    'C:\Users\Public\Documents\baseline-sample.txt',
    '#2e8f66 · baseline color sample',
    '基线样本 · 中文排版回退链检查 0123456789'
)

# ---------------------------------------------------------------- capture helpers
# 窗口刚显示时 DWM 扩展边框还没套上，Frame 会先返回客户区尺寸（324x480），
# 照这个矩形截会把右侧 16px 切掉。等边界连续几拍不变再截。
function Get-StableFrame {
    param([IntPtr]$Handle)
    $prev = ''
    for ($i = 0; $i -lt 12; $i++) {
        $f = [UiBaseline]::Frame($Handle)
        $key = "$($f.L),$($f.T),$($f.R),$($f.B)"
        if ($i -ge 2 -and $key -eq $prev) { return $f }
        $prev = $key
        Start-Sleep -Milliseconds 200
    }
    return [UiBaseline]::Frame($Handle)
}

function Save-WindowShot {
    param([IntPtr]$Handle, [string]$Path)
    $f = Get-StableFrame $Handle
    $w = $f.R - $f.L; $h = $f.B - $f.T
    if ($w -le 0 -or $h -le 0) { throw "窗口尺寸无效: ${w}x${h}" }
    # 置顶兜底：SetForegroundWindow 常被 Windows 前台锁拒绝，一旦窗口被别的
    # 应用盖住，CopyFromScreen 抓到的就是别人的界面（实测抓到过 IDE 源码）。
    # SWP_NOSIZE|SWP_NOMOVE|SWP_NOACTIVATE
    [UiBaseline]::SetWindowPos($Handle, [IntPtr](-1), 0, 0, 0, 0, 0x13) | Out-Null
    Start-Sleep -Milliseconds 350
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    try {
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        try { $g.CopyFromScreen($f.L, $f.T, 0, 0, (New-Object System.Drawing.Size($w, $h))) }
        finally { $g.Dispose() }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
        $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $bmp.Dispose()
        [UiBaseline]::SetWindowPos($Handle, [IntPtr](-2), 0, 0, 0, 0, 0x13) | Out-Null
    }
    # manifest 里记仓库相对路径，换机器/换目录仍能对上号。
    $rel = ($Path.Substring($repo.Length + 1) -replace '\\', '/')
    return [pscustomobject]@{ Path = $rel; Width = $w; Height = $h; X = $f.L; Y = $f.T }
}

function Get-VariedRatio {
    param([string]$Path)
    $bmp = [System.Drawing.Image]::FromFile($Path)
    try {
        $bg = $bmp.GetPixel(0, 0)
        $diff = 0; $step = 4; $n = 0
        for ($x = 0; $x -lt $bmp.Width; $x += $step) {
            for ($y = 0; $y -lt $bmp.Height; $y += $step) {
                $p = $bmp.GetPixel($x, $y); $n++
                if (([math]::Abs($p.R - $bg.R) + [math]::Abs($p.G - $bg.G) + [math]::Abs($p.B - $bg.B)) -gt 12) { $diff++ }
            }
        }
        if ($bg.R -eq 0 -and $bg.G -eq 0 -and $bg.B -eq 0 -and $diff -eq 0) { return 0 }
        return [math]::Round($diff * 100.0 / $n, 1)
    }
    finally { $bmp.Dispose() }
}

# ---------------------------------------------------------------- run
function Start-ClippiOnce {
    param([string[]]$ExtraArgs = @())
    $sp = @{ FilePath = (Join-Path $runDir 'clippi.exe'); PassThru = $true }
    if ($ExtraArgs.Count -gt 0) { $sp.ArgumentList = $ExtraArgs }
    $p = Start-Process @sp
    $deadline = (Get-Date).AddSeconds(15)
    $h = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline) {
        $h = [UiBaseline]::FindMain([uint32]$p.Id, 'Clippi (dev)')
        if ($h -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 150
    }
    if ($h -eq [IntPtr]::Zero) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        throw '主窗未创建（15s 超时），看 target/ui-baseline/clippi.log'
    }
    [pscustomobject]@{ Process = $p; Handle = $h }
}

# 标签只存在于数据库里，配置无法凭空造出侧栏；`pinned_tag_ids` 已指向 1..3。
function Add-BaselineTags {
    param([string]$DbPath)
    if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
        Write-Note 'python 不可用：跳过标签种子，侧栏会缺失'
        return
    }
    $py = Join-Path $runDir 'seed_tags.py'
    # 纯 ASCII 源文件，中文用 \u 转义：Set-Content -Encoding ASCII 会把字面
    # 中文折成 "??"，三行同名还会被 tags 的唯一约束当成重复全部丢弃。
    @'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.executemany(
    "insert or ignore into tags (id, uid, name, color, updated_at) values (?,?,?,?,?)",
    [(1, "baseline-work", "\u5de5\u4f5c", "", ""),
     (2, "baseline-code", "\u4ee3\u7801", "", ""),
     (3, "baseline-fav", "\u6536\u85cf", "", "")],
)
db.commit()
db.close()
'@ | Set-Content -Path $py -Encoding ASCII
    & python $py $DbPath
    Remove-Item $py -Force
}

$proc = $null
$result = [ordered]@{ theme = $Theme; started = (Get-Date).ToString('o'); shots = @() }
try {
    # 先占住剪贴板：便携实例启动瞬间会采集当前剪贴板内容，不能让它拿到用户的真实数据。
    Set-Clipboard -Value $seedTexts[0]

    Write-Host '[3/6] 初始化便携库并注入测试数据'
    $first = Start-ClippiOnce
    Start-Sleep -Milliseconds 800
    foreach ($text in $seedTexts[1..($seedTexts.Count - 1)]) {
        Set-Clipboard -Value $text
        Start-Sleep -Milliseconds 260
    }
    try {
        Add-Type -AssemblyName System.Windows.Forms
        $bmp = New-Object System.Drawing.Bitmap(320, 180)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.Clear([System.Drawing.Color]::FromArgb(46, 122, 85))
        $g.FillRectangle([System.Drawing.Brushes]::White, 24, 24, 120, 60)
        $g.Dispose()
        [System.Windows.Forms.Clipboard]::SetImage($bmp)
        $bmp.Dispose()
        Start-Sleep -Milliseconds 400
    }
    catch { Write-Note "图片注入跳过: $($_.Exception.Message)" }
    Start-Sleep -Milliseconds 1200
    Stop-Process -Id $first.Process.Id -Force
    Wait-Process -Id $first.Process.Id -Timeout 5 -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 400
    Add-BaselineTags -DbPath (Join-Path $runDir 'clippi.db')

    # 重启后再抓图：刚写过剪贴板时主线程忙着入库，全局热键会漏事件。
    Write-Host '[4/6] 重启闲置实例'
    # --restart 跳过单实例端口检测：上一个进程刚退出，端口可能仍被 TIME_WAIT 占着。
    $second = Start-ClippiOnce -ExtraArgs @('--restart')
    $proc = $second.Process
    $main = $second.Handle
    Write-Note ([UiBaseline]::Describe($main))
    # 主窗在 +1.2s 就存在，但全局热键要到 +1.9s 才注册完；过早发送会被丢弃。
    Start-Sleep -Milliseconds 2400

    Write-Host '[5/6] 抓图'
    # 快捷窗先截：主窗可见/聚焦时 Alt+C 偶发不触发，主窗隐藏时稳定。
    $quick = [UiBaseline]::FindContains([uint32]$proc.Id, 'Quick Paste')
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if ([UiBaseline]::WaitVisible($quick, 100)) { break }
        [UiBaseline]::SetCursorPos([int]($screenW / 2), [int]($screenH / 2)) | Out-Null
        Start-Sleep -Milliseconds 250
        [UiBaseline]::AltKey([byte][char]'C')
        if ([UiBaseline]::WaitVisible($quick, 1500)) { break }
        Write-Note "quick 第 $attempt 次未响应，重试"
        Start-Sleep -Milliseconds 600
    }
    if ($quick -ne [IntPtr]::Zero -and [UiBaseline]::IsWindowVisible($quick)) {
        Start-Sleep -Milliseconds 700
        $dpi = [UiBaseline]::GetDpiForWindow($quick)
        $pct = [int][math]::Round($dpi / 96.0 * 100)
        if ($ExpectDpi -gt 0 -and $dpi -ne $ExpectDpi) {
            throw "实际 DPI=$dpi ($pct%)，与 -ExpectDpi $ExpectDpi 不符。先改 Windows 显示缩放再重跑。"
        }
        $dir = Join-Path (Join-Path $OutRoot "$pct") $Theme
        Write-Note "dpi=$dpi ($pct%)  out=$dir"
        $shot = Save-WindowShot $quick (Join-Path $dir 'quick.png')
        $result.shots += $shot
        Write-Note ("quick   {0}x{1} varied={2}%" -f $shot.Width, $shot.Height, (Get-VariedRatio $shot.Path))
        [UiBaseline]::Escape()
        Start-Sleep -Milliseconds 400
    }
    else { throw 'quick 未出现：quick_hotkey_enabled / Alt+C 热键未生效' }

    # 光标挪到右下角：主窗居中，停在卡片上会让工具条浮出来污染基准图。
    [UiBaseline]::SetCursorPos(($screenW - 30), ($screenH - 30)) | Out-Null
    Start-Sleep -Milliseconds 200
    [UiBaseline]::AltKey([byte][char]'V')
    if (-not [UiBaseline]::WaitVisible($main, 3000)) { throw '主窗未显示（Alt+V 热键未生效）' }
    Start-Sleep -Milliseconds 1200
    [UiBaseline]::SetForegroundWindow($main) | Out-Null
    Start-Sleep -Milliseconds 400

    $dpi = [UiBaseline]::GetDpiForWindow($main)
    $pct = [int][math]::Round($dpi / 96.0 * 100)
    if ($ExpectDpi -gt 0 -and $dpi -ne $ExpectDpi) {
        throw "实际 DPI=$dpi ($pct%)，与 -ExpectDpi $ExpectDpi 不符。先改 Windows 显示缩放再重跑。"
    }
    $dir = Join-Path (Join-Path $OutRoot "$pct") $Theme

    $shot = Save-WindowShot $main (Join-Path $dir 'main.png')
    $result.shots += $shot
    Write-Note ("main    {0}x{1} varied={2}%" -f $shot.Width, $shot.Height, (Get-VariedRatio $shot.Path))

    # 侧栏窗口按标签行数重新定位，rows=3 时高约 80 逻辑像素；低于 60 视为还没展开。
    $sidebar = [IntPtr]::Zero
    $deadline = (Get-Date).AddSeconds(3)
    while ((Get-Date) -lt $deadline) {
        $sidebar = [UiBaseline]::FindSidebar([uint32]$proc.Id, $main)
        if ($sidebar -ne [IntPtr]::Zero) {
            $f = [UiBaseline]::Frame($sidebar)
            if (($f.B - $f.T) -ge 60) { break }
        }
        Start-Sleep -Milliseconds 200
        $sidebar = [IntPtr]::Zero
    }
    if ($sidebar -ne [IntPtr]::Zero) {
        $others = @()
        if ($IsolateSidebar) {
            $others = [UiBaseline]::Others([uint32]$proc.Id)
            foreach ($h in $others) { [UiBaseline]::ShowWindow($h, 6) | Out-Null }
            Start-Sleep -Milliseconds 700
        }
        else { Write-Note 'sidebar 背景未隔离（未加 -IsolateSidebar），对照时只看标签本身' }
        try {
            $shot = Save-WindowShot $sidebar (Join-Path $dir 'sidebar.png')
            $result.shots += $shot
            Write-Note ("sidebar {0}x{1} varied={2}%" -f $shot.Width, $shot.Height, (Get-VariedRatio $shot.Path))
        }
        finally {
            foreach ($h in $others) { [UiBaseline]::ShowWindow($h, 9) | Out-Null }
        }
    }
    else { Write-Note 'sidebar 未出现或未展开（跳过）' }

    [UiBaseline]::AltKey([byte][char]'V')
    Start-Sleep -Milliseconds 300

    $result.dpi = $dpi
    $result.dpiPercent = $pct
    $result.exeBuilt = (Get-Item $exe).LastWriteTime.ToString('o')
    $result.git = (& git -C $repo rev-parse --short HEAD 2>$null)
    $result.log = (Join-Path $runDir 'clippi.log')
    $manifestDir = Join-Path (Join-Path $OutRoot "$pct") $Theme
    New-Item -ItemType Directory -Force -Path $manifestDir | Out-Null
    $result | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $manifestDir 'manifest.json') -Encoding UTF8
    Write-Note "manifest -> $manifestDir\manifest.json"
}
finally {
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    if ($null -ne $savedClip) { try { Set-Clipboard -Value $savedClip } catch { } }
    Write-Host '[6/6] 完成；便携数据保留在 target/ui-baseline（可重复运行覆盖）'
}
