$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (& (Join-Path $root "target\$env:TARGET\release\zi.exe") --version).Split(' ')[1]
$installer = Join-Path $root "dist\zistral-v$version-windows-x64-setup.exe"
$destination = Join-Path $env:RUNNER_TEMP 'zi-install-smoke'
$originalPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$config = Join-Path $env:APPDATA 'zistral\config.toml'
$originalConfig = if (Test-Path -LiteralPath $config) { [IO.File]::ReadAllBytes($config) } else { $null }
try {
    # Install and upgrade must not duplicate the PATH entry or touch model settings.
    1..2 | ForEach-Object {
        $p = Start-Process -FilePath $installer -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$destination`"") -WindowStyle Hidden -Wait -PassThru
        if ($p.ExitCode -ne 0) { throw "Installer exited $($p.ExitCode)" }
        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if (@($userPath.Split(';') | Where-Object { $_ -eq $destination }).Count -ne 1) { throw 'PATH entry missing or duplicated' }
        if ((& "$destination\zi.exe" --version) -ne "zi $version") { throw 'Installed command failed' }
        $count = 'hello world' | & "$destination\zi.exe" --count | ConvertFrom-Json
        if ($count.words -ne 2) { throw 'Installed pipe input failed' }
    }
    if (-not (Test-Path -LiteralPath "$destination\launch.cmd")) { throw 'Start menu launcher missing' }
} finally {
    if (Test-Path -LiteralPath "$destination\unins000.exe") {
        $p = Start-Process -FilePath "$destination\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -WindowStyle Hidden -Wait -PassThru
        if ($p.ExitCode -ne 0) { throw "Uninstaller exited $($p.ExitCode)" }
    }
}
if ([Environment]::GetEnvironmentVariable('Path', 'User') -ne $originalPath) { throw 'Uninstall changed unrelated PATH entries' }
if ($null -eq $originalConfig) {
    if (Test-Path -LiteralPath $config) { throw 'Installer unexpectedly created model configuration' }
} elseif ([Convert]::ToBase64String([IO.File]::ReadAllBytes($config)) -ne [Convert]::ToBase64String($originalConfig)) { throw 'Model configuration was modified' }
Write-Output 'Installer, upgrade, command, pipe, uninstall, PATH and config preservation passed.'
