Add-Type -AssemblyName System.Drawing
$ErrorActionPreference = "Stop"

$iconDir = Join-Path $PSScriptRoot "..\src-tauri\icons"
New-Item -ItemType Directory -Force -Path $iconDir | Out-Null

function New-LumaBitmap([int]$size) {
    $bitmap = New-Object System.Drawing.Bitmap($size, $size)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([System.Drawing.Color]::Transparent)

    $margin = [Math]::Max(1, [int]($size * 0.06))
    $radius = [single]($size * 0.25)
    $side = [single]($size - 2 * $margin)
    $rect = [System.Drawing.RectangleF]::new([single]$margin, [single]$margin, $side, $side)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $diameter = 2 * $radius
    $path.AddArc($rect.X, $rect.Y, $diameter, $diameter, 180, 90)
    $path.AddArc($rect.Right - $diameter, $rect.Y, $diameter, $diameter, 270, 90)
    $path.AddArc($rect.Right - $diameter, $rect.Bottom - $diameter, $diameter, $diameter, 0, 90)
    $path.AddArc($rect.X, $rect.Bottom - $diameter, $diameter, $diameter, 90, 90)
    $path.CloseFigure()

    $brush = [System.Drawing.SolidBrush]::new(
        [System.Drawing.Color]::FromArgb(255, 10, 100, 240)
    )
    $graphics.FillPath($brush, $path)

    $fontSize = [single]($size * 0.48)
    $font = New-Object System.Drawing.Font("Microsoft YaHei UI", $fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $format = New-Object System.Drawing.StringFormat
    $format.Alignment = [System.Drawing.StringAlignment]::Center
    $format.LineAlignment = [System.Drawing.StringAlignment]::Center
    $textBrush = [System.Drawing.Brushes]::White
    $glyph = ([char]0x8BD1).ToString()
    $graphics.DrawString($glyph, $font, $textBrush, $rect, $format)

    $font.Dispose()
    $format.Dispose()
    $brush.Dispose()
    $path.Dispose()
    $graphics.Dispose()
    return $bitmap
}

foreach ($size in @(32, 128)) {
    $bitmap = New-LumaBitmap $size
    $bitmap.Save((Join-Path $iconDir "$size`x$size.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}

$large = New-LumaBitmap 512
$large.Save((Join-Path $iconDir "icon.png"), [System.Drawing.Imaging.ImageFormat]::Png)
$handle = $large.GetHicon()
$icon = [System.Drawing.Icon]::FromHandle($handle)
$stream = [System.IO.File]::Create((Join-Path $iconDir "icon.ico"))
$icon.Save($stream)
$stream.Dispose()
$icon.Dispose()
$large.Dispose()
