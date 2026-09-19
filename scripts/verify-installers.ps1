$ErrorActionPreference = 'Stop'
$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$expectedVersion = (Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw | ConvertFrom-Json).version
$msi = Join-Path $projectRoot "src-tauri/target/release/bundle/msi/LumaRoute_${expectedVersion}_x64_zh-CN.msi"
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($msi, 0)
$properties = $database.OpenView('SELECT `Property`, `Value` FROM `Property`')
$properties.Execute()
$version = ''
while ($record = $properties.Fetch()) { if ($record.StringData(1) -eq 'ProductVersion') { $version = $record.StringData(2) } }
$properties.Close()
if ($version -ne $expectedVersion) { throw "Unexpected MSI version: $version" }
$view = $database.OpenView('SELECT `FileName`, `FileSize` FROM `File`')
$view.Execute()
$files = @()
while ($record = $view.Fetch()) { $files += [pscustomobject]@{name=$record.StringData(1);size=$record.IntegerData(2)} }
$view.Close()
foreach ($required in @('lumaroute.exe','xray.exe','sing-box.exe','libcronet.dll','wintun.dll','LICENSE-wintun.txt','geoip.dat','geosite.dat')) {
    if (!($files | Where-Object { ($_.name -split '\|')[-1] -eq $required })) { throw "MSI missing $required" }
}
$report = [pscustomobject]@{version=$version;mode='MSI database read-only; no installation';files=$files}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $projectRoot 'test-artifacts/msi-contents.json')
Write-Host "MSI $version verified: main executable, both cores, Wintun, licenses, and geo data are present."
