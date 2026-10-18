; GridCraft per-user installer (Inno Setup 6): installs without administrator rights.
;
;   iscc /DVersion=0.7.0 /DBinDir=<folder with gridcraft.exe and gridcraft-cli.exe> /DOutDir=dist gridcraft-user.iss
;
; Installs into %LOCALAPPDATA%\Programs\GridCraft for the current user only, with a Start Menu
; shortcut, an optional desktop shortcut, App Paths (Win+R `gridcraft`) and GridCraft offered
; under "Open with" for spreadsheet files, all in HKEY_CURRENT_USER. Uninstall from Settings >
; Apps. The binaries link the C runtime statically, so nothing else is needed.
;
; AppId is fixed for this per-user installer (generated 2026-10-18): never change it, or upgrades
; install side by side. It differs from the MSI's UpgradeCode on purpose: the per-machine MSI and
; this per-user install are separate products.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef BinDir
  #error Pass /DBinDir=<folder with gridcraft.exe and gridcraft-cli.exe>
#endif
#ifndef OutDir
  #define OutDir "."
#endif

[Setup]
AppId={{D7C0FF5B-F687-4EAA-8018-CAB8E9075217}
AppName=GridCraft
AppVersion={#Version}
AppVerName=GridCraft {#Version}
AppPublisher=Learning Machines LLC
AppPublisherURL=https://getartcraft.com/apps/gridcraft
AppSupportURL=https://discord.gg/artcraft
AppUpdatesURL=https://getartcraft.com/apps/gridcraft
; Per-user: no UAC prompt; {autopf} resolves to %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
DefaultDirName={autopf}\GridCraft
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SetupIconFile=..\..\assets\app-icon\gridcraft.ico
UninstallDisplayIcon={app}\gridcraft.exe
UninstallDisplayName=GridCraft
LicenseFile=..\..\LICENSE-MIT
OutputDir={#OutDir}
OutputBaseFilename=gridcraft-{#Version}-windows-x64-user-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinDir}\gridcraft.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\gridcraft-cli.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\GridCraft"; Filename: "{app}\gridcraft.exe"
Name: "{autodesktop}\GridCraft"; Filename: "{app}\gridcraft.exe"; Tasks: desktopicon

[Registry]
; Win+R `gridcraft`.
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\gridcraft.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\gridcraft.exe"; Flags: uninsdeletekey
; "Open with" for spreadsheet files, without taking over the user's default app.
Root: HKA; Subkey: "Software\Classes\GridCraft.Workbook"; ValueType: string; ValueName: ""; ValueData: "GridCraft Workbook"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\GridCraft.Workbook\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\gridcraft.exe,0"
Root: HKA; Subkey: "Software\Classes\GridCraft.Workbook\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\gridcraft.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\.xlsx\OpenWithProgids"; ValueType: string; ValueName: "GridCraft.Workbook"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.xlsm\OpenWithProgids"; ValueType: string; ValueName: "GridCraft.Workbook"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.xlsb\OpenWithProgids"; ValueType: string; ValueName: "GridCraft.Workbook"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.ods\OpenWithProgids"; ValueType: string; ValueName: "GridCraft.Workbook"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.csv\OpenWithProgids"; ValueType: string; ValueName: "GridCraft.Workbook"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.tsv\OpenWithProgids"; ValueType: string; ValueName: "GridCraft.Workbook"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\gridcraft.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "GridCraft"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\gridcraft.exe\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\gridcraft.exe"" ""%1"""

[Run]
Filename: "{app}\gridcraft.exe"; Description: "{cm:LaunchProgram,GridCraft}"; Flags: nowait postinstall skipifsilent
