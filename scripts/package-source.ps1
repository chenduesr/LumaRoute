param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$version = (Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw | ConvertFrom-Json).version
if (!$OutputDirectory) { $OutputDirectory = Join-Path (Split-Path $projectRoot -Parent) "releases/$version" }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$archivePath = Join-Path $OutputDirectory "MyRay-Lite-$version-source.zip"
$rootFiles = @('.gitignore','.prettierignore','LICENSE','README.md','index.html','package.json','package-lock.json','tsconfig.json','tsconfig.node.json','vite.config.ts')
$directories = @('src','public','scripts','docs','tests/fixtures','src-tauri/src','src-tauri/icons','src-tauri/capabilities','src-tauri/resources')
$files = @()
foreach ($name in $rootFiles + @('src-tauri/Cargo.toml','src-tauri/Cargo.lock','src-tauri/build.rs','src-tauri/tauri.conf.json','src-tauri/.gitignore')) {
    $path = Join-Path $projectRoot $name
    if (Test-Path -LiteralPath $path -PathType Leaf) { $files += Get-Item -LiteralPath $path }
}
foreach ($name in $directories) {
    $path = Join-Path $projectRoot $name
    if (Test-Path -LiteralPath $path) { $files += Get-ChildItem -LiteralPath $path -Recurse -File }
}
$files = @($files | Sort-Object FullName -Unique)
$prefix = $projectRoot.TrimEnd('\') + '\'
$stream = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::Create, [System.IO.FileAccess]::ReadWrite)
$zip = New-Object System.IO.Compression.ZipArchive($stream, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in $files) {
        if (!$file.FullName.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) { throw 'Source path outside project' }
        if ($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { throw "Refusing linked file: $($file.Name)" }
        $relative = $file.FullName.Substring($prefix.Length).Replace('\','/')
        $entryName = "MyRay-Lite-$version/$relative"
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName, $entryName, [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally { $zip.Dispose(); $stream.Dispose() }
# Read back and hash every entry against its source, rather than only checking ZIP creation.
$zip = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
try {
    if ($zip.Entries.Count -ne $files.Count) { throw 'Source archive file count mismatch' }
    foreach ($entry in $zip.Entries) {
        $relative = $entry.FullName.Substring("MyRay-Lite-$version/".Length)
        $source = [System.IO.File]::OpenRead((Join-Path $projectRoot $relative))
        $packed = $entry.Open()
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
            $a = [System.BitConverter]::ToString($sha.ComputeHash($source))
            $b = [System.BitConverter]::ToString($sha.ComputeHash($packed))
            if ($a -ne $b) { throw "Source archive mismatch: $relative" }
        } finally { $source.Dispose(); $packed.Dispose(); $sha.Dispose() }
    }
} finally { $zip.Dispose() }
Write-Host "Verified $($files.Count) source/resource files: $archivePath"
