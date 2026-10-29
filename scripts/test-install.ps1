param([string]$InstallerPath,[string]$BinaryPath,[string]$Destination)
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
}
function Uninstall-Candidate {
    if(Test-Path -LiteralPath "$Destination\unins000.exe"){
        $p=Start-Process -FilePath "$Destination\unins000.exe" -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -WindowStyle Hidden -Wait -PassThru
        if($p.ExitCode -ne 0){throw "Uninstaller exited $($p.ExitCode)"}
    }
}
try{
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
if($null -eq $originalConfig){
    if(Test-Path -LiteralPath $config){throw 'Installer unexpectedly created model configuration'}
}elseif([Convert]::ToBase64String([IO.File]::ReadAllBytes($config)) -ne [Convert]::ToBase64String($originalConfig)){throw 'Model configuration was modified'}
Write-Output 'Unicode/space path: install twice, legacy duplicate repair, user-owned entry preservation, executable equality, pipe, uninstall and config checks passed.'
