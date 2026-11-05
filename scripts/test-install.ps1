param([string]$InstallerPath,[string]$BinaryPath,[string]$Destination,[string]$BaselineAssets)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
if(-not $BinaryPath){$BinaryPath=Join-Path $root "target\$env:TARGET\release\zi.exe"}
$version=(& $BinaryPath --version).Split(' ')[1]
if(-not $InstallerPath){$InstallerPath=Join-Path $root "dist\zistral-v$version-windows-x64-setup.exe"}
if(-not $Destination){$Destination=Join-Path $env:RUNNER_TEMP 'zi install-测试'}
$Destination=[IO.Path]::GetFullPath($Destination)
$registration='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{DC6B24E8-244C-44B5-8389-CA94DFEB92AC}_is1'
if(Test-Path -LiteralPath $registration){throw 'Preserve existing native installation; use a clean test account.'}
if(Test-Path -LiteralPath $Destination){throw 'Use a new test directory; preserve existing files.'}
if(Get-ItemPropertyValue -LiteralPath 'HKCU:\Software\Zistral' -Name OwnedPath -ErrorAction SilentlyContinue){throw 'Preserve existing native PATH ownership; use a clean test account.'}
$environmentKey=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
try{$originalKind=$environmentKey.GetValueKind('Path');$originalPath=$environmentKey.GetValue('Path',$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}finally{$environmentKey.Dispose()}
$config=Join-Path $env:APPDATA 'zistral\config.toml'
$originalConfig=if(Test-Path -LiteralPath $config){[IO.File]::ReadAllBytes($config)}else{$null}
$baseline=$null
$fixtureCreated=$false
$configDirectory=Split-Path -Parent $config
$sentinel=[Text.Encoding]::UTF8.GetBytes("# Stable-compatible upgrade fixture; preserve bytes.`nmodel='stable-root-model'`nbase_url='http://127.0.0.1:1/v1'`napi_key='SYNTHETIC_ROOT_SENTINEL_993'`nactive_profile='work'`n[profiles.work]`nmodel='stable-work-model'`nbase_url='http://127.0.0.1:1/v1'`napi='responses'`napi_key_env='ZISTRAL_UPGRADE_UNSET_993'`napi_key='SYNTHETIC_WORK_SENTINEL_993'`nendpoint='http://127.0.0.1:1/v1/responses'`n[profiles.other]`nmodel='stable-other-model'`napi_key='SYNTHETIC_OTHER_SENTINEL_993'`n")
if($BaselineAssets){
    if(Test-Path -LiteralPath $configDirectory){throw 'Preserve existing model configuration directory; use a clean test account.'}
    $baseline=& python (Join-Path $root 'scripts/upgrade_baseline.py') --assets $BaselineAssets --target x86_64-pc-windows-msvc | ConvertFrom-Json
    if($LASTEXITCODE -ne 0 -or -not $baseline.installer){throw 'Published baseline verification failed; no installation attempted.'}
}
function Check-LegacyConfiguration {
    if($baseline){
        if([Convert]::ToBase64String([IO.File]::ReadAllBytes($config)) -ne [Convert]::ToBase64String($sentinel)){throw 'Complete legacy model configuration changed'}
        $models=& "$Destination\zi.exe" --config $config models
        if($LASTEXITCODE -ne 0 -or "$models" -notmatch 'stable-work-model'){throw 'Installed executable cannot read legacy configuration'}
    }
}
function Read-UserPath {
    $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try{$key.GetValue('Path',$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}finally{$key.Dispose()}
}
function Write-UserPath([string]$Value) {
    $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$true)
    try{$key.SetValue('Path',$Value,$originalKind)}finally{$key.Dispose()}
}
function Count-TestPath {
    @((Read-UserPath).Split(';')|Where-Object {$_.Trim().TrimEnd('\') -eq $Destination.TrimEnd('\')}).Count
}
function Install-Candidate {
    $p=Start-Process -FilePath $InstallerPath -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOCLOSEAPPLICATIONS','/NORESTARTAPPLICATIONS',"/DIR=`"$Destination`"") -WindowStyle Hidden -Wait -PassThru
    if($p.ExitCode -ne 0){throw "Installer exited $($p.ExitCode)"}
    if((Count-TestPath) -ne 1){throw 'PATH entry missing or duplicated'}
    if((Get-FileHash -LiteralPath "$Destination\zi.exe").Hash -ne (Get-FileHash -LiteralPath $BinaryPath).Hash){throw 'Installed executable differs from packaged source'}
    if((& "$Destination\zi.exe" --version) -ne "zi $version"){throw 'Installed command failed'}
    $count='hello world' | & "$Destination\zi.exe" --count | ConvertFrom-Json
    if($count.words -ne 2){throw 'Installed pipe input failed'}
    if(-not(Test-Path -LiteralPath "$Destination\launch.cmd")){throw 'Start menu launcher missing'}
    Check-LegacyConfiguration
    & python (Join-Path $root 'scripts/test_pre_sharing_delivery.py') --binary "$Destination\zi.exe"
    if($LASTEXITCODE -ne 0){throw 'Installed pre-sharing workflow failed'}
    & python (Join-Path $root 'scripts/test_table_preparation_delivery.py') --binary "$Destination\zi.exe"
    if($LASTEXITCODE -ne 0){throw 'Installed table-preparation workflow failed'}
    & python (Join-Path $root 'scripts/test_service_setup_delivery.py') --binary "$Destination\zi.exe"
    if($LASTEXITCODE -ne 0){throw 'Installed service-setup workflow failed'}
}
function Uninstall-Candidate {
    if(Test-Path -LiteralPath "$Destination\unins000.exe"){
        $p=Start-Process -FilePath "$Destination\unins000.exe" -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -WindowStyle Hidden -Wait -PassThru
        if($p.ExitCode -ne 0){throw "Uninstaller exited $($p.ExitCode)"}
    }
}
try {
try{
    if($baseline){
        New-Item -ItemType Directory -Path $configDirectory | Out-Null
        $fixtureCreated=$true
        [IO.File]::WriteAllBytes($config,$sentinel)
        $p=Start-Process -FilePath $baseline.installer -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOCLOSEAPPLICATIONS','/NORESTARTAPPLICATIONS',"/DIR=`"$Destination`"") -WindowStyle Hidden -Wait -PassThru
        if($p.ExitCode -ne 0){throw "Published installer exited $($p.ExitCode)"}
        if((Count-TestPath) -ne 1){throw 'Published installer PATH missing or duplicated'}
        if((Get-FileHash -LiteralPath "$Destination\zi.exe").Hash.ToLowerInvariant() -ne $baseline.binary_sha256){throw 'Published executable differs from pinned release'}
        if((& "$Destination\zi.exe" --version) -ne "zi $($baseline.version)"){throw 'Published command version differs'}
        Check-LegacyConfiguration
    }
    Install-Candidate
    Install-Candidate
    # Simulate the owned duplicate entries created by an older installer.
    Write-UserPath ($Destination+';'+(Read-UserPath))
    if((Count-TestPath) -ne 2){throw 'Legacy duplicate fixture was not created'}
    Install-Candidate
}finally{Uninstall-Candidate}
if((Read-UserPath) -ne $originalPath){throw 'Owned-path upgrade/uninstall changed unrelated PATH entries'}

# A user-provided equivalent PATH entry must remain user-owned across installs.
$userEntry=$Destination.ToUpperInvariant()+'\'
$seededPath=$userEntry+';'+$originalPath
Write-UserPath $seededPath
try{
    Install-Candidate
    Install-Candidate
    if((Read-UserPath) -ne $seededPath){throw 'Installer rewrote a user-provided PATH entry'}
    if(Get-ItemPropertyValue -LiteralPath 'HKCU:\Software\Zistral' -Name OwnedPath -ErrorAction SilentlyContinue){throw 'Installer claimed a pre-existing user PATH entry'}
}finally{
    Uninstall-Candidate
    if((Read-UserPath) -ne $seededPath){throw 'Uninstall changed the pre-existing user PATH entry'}
    Write-UserPath $originalPath
}
if((Read-UserPath) -ne $originalPath){throw 'Original PATH not restored'}
if(-not $baseline -and $null -eq $originalConfig){
    if(Test-Path -LiteralPath $config){throw 'Installer unexpectedly created model configuration'}
}elseif(-not $baseline -and [Convert]::ToBase64String([IO.File]::ReadAllBytes($config)) -ne [Convert]::ToBase64String($originalConfig)){throw 'Model configuration was modified'}
} finally {
    if($fixtureCreated){
        if(-not(Test-Path -LiteralPath $config) -or [Convert]::ToBase64String([IO.File]::ReadAllBytes($config)) -ne [Convert]::ToBase64String($sentinel)){throw 'Preserve unexpected model configuration; fixture cleanup refused'}
        [IO.File]::Delete($config)
        [IO.Directory]::Delete($configDirectory,$false)
    }
}
if($baseline){
    [ordered]@{passed=1;published_baseline=$baseline;candidate_version=$version;package_version_changed=($baseline.version -ne $version);configuration_preserved=$true;baseline_installs=1} | ConvertTo-Json -Depth 5 -Compress
}
Write-Output 'Unicode/space path: install twice, legacy duplicate repair, user-owned entry preservation, executable equality, pipe, uninstall and config checks passed.'
