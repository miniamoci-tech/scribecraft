; Inno Setup script for the ScribeCraft Windows installer. Built by `cargo xtask dist`,
; which passes AppVersion, StageDir and OutputDir. Per-user install: no administrator rights.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{5B7E3C1A-8F2D-4C6B-9A1E-2D4F6A8C0B13}
AppName=ScribeCraft
AppVersion={#AppVersion}
AppPublisher=The ScribeCraft contributors
AppPublisherURL=https://github.com/miniamoci-tech/scribecraft
DefaultDirName={autopf}\ScribeCraft
DefaultGroupName=ScribeCraft
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename=ScribeCraft-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile={#StageDir}\LICENSE-MIT
UninstallDisplayIcon={app}\scribecraft.exe
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#StageDir}\scribecraft.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\scribecraft-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\LICENSE-MIT"; DestDir: "{app}"
Source: "{#StageDir}\LICENSE-APACHE"; DestDir: "{app}"
Source: "{#StageDir}\NOTICE"; DestDir: "{app}"
Source: "{#StageDir}\README-install.md"; DestDir: "{app}"

[Icons]
Name: "{group}\ScribeCraft"; Filename: "{app}\scribecraft.exe"
Name: "{group}\{cm:UninstallProgram,ScribeCraft}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\ScribeCraft"; Filename: "{app}\scribecraft.exe"; Tasks: desktopicon

[Registry]
; "Open with ScribeCraft" for common recordings, without taking over the default player.
; The parent key carries the delete flag so uninstalling removes the whole entry, not just its leaf.
Root: HKA; Subkey: "Software\Classes\Applications\scribecraft.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\scribecraft.exe\shell\open\command"; ValueType: string; ValueData: """{app}\scribecraft.exe"" ""%1"""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\.wav\OpenWithList\scribecraft.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\.mp3\OpenWithList\scribecraft.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\.m4a\OpenWithList\scribecraft.exe"; Flags: uninsdeletekey

[Run]
Filename: "{app}\scribecraft.exe"; Description: "{cm:LaunchProgram,ScribeCraft}"; Flags: nowait postinstall skipifsilent
