Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = "Stop"

$iconsDir = Join-Path $PSScriptRoot "..\src-tauri\icons"
if (-not (Test-Path $iconsDir)) {
    throw "Icons directory not found: $iconsDir"
}
$iconsDir = (Resolve-Path $iconsDir).Path

$svgPath = Join-Path $iconsDir "icon.svg"
if (-not (Test-Path $svgPath)) {
    throw "Source SVG not found: $svgPath"
}

function Get-SvgEmbeddedImage([string]$path) {
    $content = Get-Content -LiteralPath $path -Raw
    $pattern = 'data:image/(?<fmt>png|jpeg|jpg|webp);base64,(?<data>[A-Za-z0-9+/=]+)'
    $match = [regex]::Match($content, $pattern)
    if (-not $match.Success) {
        throw "icon.svg does not contain an embedded base64 bitmap."
    }

    $bytes = [Convert]::FromBase64String($match.Groups["data"].Value)
    $stream = New-Object System.IO.MemoryStream(, $bytes)
    return [System.Drawing.Bitmap]::FromStream($stream)
}

function Resize-Bitmap([System.Drawing.Bitmap]$src, [int]$w, [int]$h) {
    $dst = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($dst)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.DrawImage($src, (New-Object System.Drawing.Rectangle(0, 0, $w, $h)))
    $graphics.Dispose()
    return $dst
}

function Save-Png([System.Drawing.Bitmap]$bitmap, [string]$path) {
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Output "  -> $path"
}

Write-Output "Generating MomoBako icons from $svgPath"

$source = Get-SvgEmbeddedImage $svgPath
Write-Output "  source: $($source.Width) x $($source.Height)"

$icon1024 = Resize-Bitmap $source 1024 1024
Save-Png $icon1024 (Join-Path $iconsDir "icon-source.png")
Save-Png $icon1024 (Join-Path $iconsDir "icon.png")

$icon256 = Resize-Bitmap $source 256 256
Save-Png $icon256 (Join-Path $iconsDir "128x128@2x.png")

$icon128 = Resize-Bitmap $source 128 128
Save-Png $icon128 (Join-Path $iconsDir "128x128.png")

$icon32 = Resize-Bitmap $source 32 32
Save-Png $icon32 (Join-Path $iconsDir "32x32.png")

$icoSizes = @(16, 32, 48, 64, 128, 256)
$icoBitmaps = @{}
foreach ($size in $icoSizes) {
    $icoBitmaps[$size] = Resize-Bitmap $source $size $size
}

$pngBytesPerSize = @{}
foreach ($size in $icoSizes) {
    $stream = New-Object System.IO.MemoryStream
    $icoBitmaps[$size].Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngBytesPerSize[$size] = $stream.ToArray()
    $stream.Dispose()
}

$icoPath = Join-Path $iconsDir "icon.ico"
$fileStream = [System.IO.File]::Open($icoPath, [System.IO.FileMode]::Create)
$writer = New-Object System.IO.BinaryWriter($fileStream)

$writer.Write([UInt16]0)
$writer.Write([UInt16]1)
$writer.Write([UInt16]$icoSizes.Count)

$headerSize = 6 + 16 * $icoSizes.Count
$offsets = @{}
$runningOffset = $headerSize
foreach ($size in $icoSizes) {
    $offsets[$size] = $runningOffset
    $runningOffset += $pngBytesPerSize[$size].Length
}

foreach ($size in $icoSizes) {
    $entrySize = if ($size -ge 256) { 0 } else { $size }
    $writer.Write([byte]$entrySize)
    $writer.Write([byte]$entrySize)
    $writer.Write([byte]0)
    $writer.Write([byte]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]32)
    $writer.Write([UInt32]$pngBytesPerSize[$size].Length)
    $writer.Write([UInt32]$offsets[$size])
}

foreach ($size in $icoSizes) {
    $writer.Write($pngBytesPerSize[$size])
}

$writer.Flush()
$writer.Dispose()
$fileStream.Dispose()
Write-Output "  -> $icoPath"

$source.Dispose()
$icon1024.Dispose()
$icon256.Dispose()
$icon128.Dispose()
$icon32.Dispose()
foreach ($size in $icoSizes) {
    $icoBitmaps[$size].Dispose()
}

Write-Output "Done."
