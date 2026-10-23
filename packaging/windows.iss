#ifndef Version
  #error Version is required
#endif
[Setup]
AppId={{DC6B24E8-244C-44B5-8389-CA94DFEB92AC}
AppName=Zistral (zi)
AppVersion={#Version}
AppPublisher=ax2
AppPublisherURL=https://github.com/ax2/zistral
AppSupportURL=https://github.com/ax2/zistral/issues
DefaultDirName={localappdata}\Programs\Zistral
DefaultGroupName=Zistral
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
OutputDir={#OutputPath}
OutputBaseFilename=zistral-v{#Version}-windows-x64-setup
UninstallDisplayName=Zistral (zi)
UninstallDisplayIcon={app}\zi.exe
LicenseFile={#RepoRoot}\LICENSE
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Files]
Source: "{#BinaryPath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\README.zh-CN.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\packaging\launch.cmd"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\zi"; Filename: "{app}\launch.cmd"; WorkingDir: "{userprofile}"; Comment: "Start zi and connect your model"
Name: "{group}\Uninstall Zistral"; Filename: "{uninstallexe}"

[Run]
Filename: "{app}\launch.cmd"; Description: "Launch zi / 启动 zi 并配置模型"; Flags: postinstall shellexec skipifsilent

[Code]
function SamePath(A, B: String): Boolean;
begin
  Result := CompareText(RemoveBackslashUnlessRoot(Trim(A)), RemoveBackslashUnlessRoot(Trim(B))) = 0;
end;

function WithoutPath(Value, Target: String): String;
var Item: String; Start, Finish, Count: Integer;
begin
  Result := Value;
  Start := 1;
  while Start <= Length(Value) do begin
    Finish := Start;
    while (Finish <= Length(Value)) and (Value[Finish] <> ';') do Finish := Finish + 1;
    Item := Copy(Value, Start, Finish - Start);
    if SamePath(Item, Target) then begin
      Count := Finish - Start;
      if Finish <= Length(Value) then Count := Count + 1
      else if Start > 1 then begin Start := Start - 1; Count := Count + 1; end;
      Delete(Result, Start, Count);
      Exit;
    end;
    Start := Finish + 1;
  end;
end;

function ContainsPath(Value, Target: String): Boolean;
var Item: String; P: Integer;
begin
  Result := False;
  while Value <> '' do begin
    P := Pos(';', Value);
    if P = 0 then begin Item := Value; Value := ''; end
    else begin Item := Copy(Value, 1, P - 1); Delete(Value, 1, P); end;
    if SamePath(Item, Target) then begin Result := True; Exit; end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var Value, AppPath, Owned: String;
begin
  if CurStep = ssPostInstall then begin
    AppPath := ExpandConstant('{app}');
    RegQueryStringValue(HKCU, 'Environment', 'Path', Value);
    RegQueryStringValue(HKCU, 'Software\Zistral', 'OwnedPath', Owned);
    if (Owned <> '') and not SamePath(Owned, AppPath) then Value := WithoutPath(Value, Owned);
    if not ContainsPath(Value, AppPath) then begin
      if Value = '' then Value := AppPath else Value := AppPath + ';' + Value;
      if not RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Value) then
        RaiseException('Cannot update your PATH. Please run zi using the Start menu shortcut.');
      RegWriteStringValue(HKCU, 'Software\Zistral', 'OwnedPath', AppPath);
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var Value, Owned: String;
begin
  if CurUninstallStep = usUninstall then begin
    if RegQueryStringValue(HKCU, 'Software\Zistral', 'OwnedPath', Owned) and SamePath(Owned, ExpandConstant('{app}')) then begin
      if RegQueryStringValue(HKCU, 'Environment', 'Path', Value) then
        RegWriteExpandStringValue(HKCU, 'Environment', 'Path', WithoutPath(Value, Owned));
      RegDeleteValue(HKCU, 'Software\Zistral', 'OwnedPath');
      RegDeleteKeyIfEmpty(HKCU, 'Software\Zistral');
    end;
  end;
end;
