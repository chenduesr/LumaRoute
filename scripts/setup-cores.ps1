$ErrorActionPreference = 'Stop'
function File-Sha256([string]$path) {
    $stream = [System.IO.File]::OpenRead($path)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { return [System.BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') }
    finally { $stream.Dispose(); $sha.Dispose() }
}
$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$lock = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'cores.lock.json') -Raw | ConvertFrom-Json
foreach ($kind in @('xray', 'singbox')) {
    $core = $lock.$kind
    $destination = Join-Path $projectRoot "src-tauri/resources/$kind"
    $valid = $true
    foreach ($f in $core.files.PSObject.Properties) {
        $target = Join-Path $destination $f.Name
        if (!(Test-Path -LiteralPath $target) -or (File-Sha256 $target) -ne $f.Value) { $valid = $false; break }
    }
    if ($valid) { Write-Host "$kind $($core.version) verified; already installed."; continue }
    $stage = Join-Path $projectRoot ('setup/core-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    $zip = Join-Path $stage 'core.zip'
    Write-Host "Downloading official $kind $($core.version)..."
    Invoke-WebRequest -Uri $core.url -OutFile $zip
    if ((File-Sha256 $zip) -ne $core.sha256) { throw "$kind archive SHA-256 mismatch; resources unchanged." }
    $extracted = Join-Path $stage 'extracted'
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($zip, $extracted)
    $verified = @{}
    foreach ($f in $core.files.PSObject.Properties) {
        $found = @(Get-ChildItem -LiteralPath $extracted -Recurse -File | Where-Object Name -EQ $f.Name)
        if ($found.Count -ne 1 -or (File-Sha256 $found[0].FullName) -ne $f.Value) { throw "$kind resource verification failed: $($f.Name)" }
        $verified[$f.Name] = $found[0].FullName
    }
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    foreach ($name in $verified.Keys) { Copy-Item -LiteralPath $verified[$name] -Destination (Join-Path $destination $name) -Force }
    Write-Host "$kind $($core.version) installed and SHA-256 verified."
}
