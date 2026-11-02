; Inno Setup 6 script: builds AudioStrada-<version>-x64-setup.exe.
; Build first with: cargo build -p audiostrada-app --profile dist --locked
; Then: iscc /DVersion=0.1.0 packaging\windows\audiostrada.iss

#ifndef Version
  #define Version "0.1.0"
#endif

[Setup]
AppId={{5cd4558f-cde1-4ea9-a20b-b4708f14594a}
AppName=AudioStrada
AppVersion={#Version}
AppPublisher=AudioStrada contributors
DefaultDirName={autopf}\AudioStrada
DefaultGroupName=AudioStrada
UninstallDisplayIcon={app}\audiostrada.exe
OutputDir=..\..\target\installer
OutputBaseFilename=AudioStrada-{#Version}-x64-setup
SetupIconFile=..\icons\audiostrada.ico
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
PrivilegesRequiredOverridesAllowed=dialog

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "association"; Description: "Open .strada projects with AudioStrada"; GroupDescription: "Files:"

[Files]
Source: "..\..\target\dist\audiostrada.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\icons\audiostrada.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\AudioStrada"; Filename: "{app}\audiostrada.exe"
Name: "{group}\Uninstall AudioStrada"; Filename: "{uninstallexe}"
Name: "{autodesktop}\AudioStrada"; Filename: "{app}\audiostrada.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.strada"; ValueType: string; ValueName: ""; ValueData: "AudioStrada.Project"; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\AudioStrada.Project"; ValueType: string; ValueName: ""; ValueData: "AudioStrada project"; Flags: uninsdeletekey; Tasks: association
Root: HKA; Subkey: "Software\Classes\AudioStrada.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\audiostrada.ico"; Tasks: association
Root: HKA; Subkey: "Software\Classes\AudioStrada.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\audiostrada.exe"" ""%1"""; Tasks: association

[Run]
Filename: "{app}\audiostrada.exe"; Description: "{cm:LaunchProgram,AudioStrada}"; Flags: nowait postinstall skipifsilent
