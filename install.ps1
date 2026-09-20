# Install the latest five-lines release on Windows.
#   irm https://raw.githubusercontent.com/OWNER/REPO/main/install.ps1 | iex
# Set $env:FIVE_LINES_REPO = "owner/repo" to install from a fork.
$ErrorActionPreference = "Stop"

$repo = if ($env:FIVE_LINES_REPO) { $env:FIVE_LINES_REPO } else { "OWNER/REPO" }
$dir = if ($env:FIVE_LINES_DIR) { $env:FIVE_LINES_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\five-lines" }
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "aarch64" } else { "x86_64" }
$target = "$arch-pc-windows-msvc"

$tag = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
$name = "five-lines-$tag-$target"
$zip = Join-Path $env:TEMP "$name.zip"
Invoke-WebRequest "https://github.com/$repo/releases/download/$tag/$name.zip" -OutFile $zip
Expand-Archive $zip -DestinationPath $env:TEMP -Force
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item (Join-Path $env:TEMP "$name\five-lines.exe") $dir -Force
Remove-Item $zip, (Join-Path $env:TEMP $name) -Recurse -Force

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $dir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$dir", "User")
    Write-Host "added $dir to your user PATH; open a new terminal to use it"
}
Write-Host "installed five-lines $tag to $dir\five-lines.exe"
