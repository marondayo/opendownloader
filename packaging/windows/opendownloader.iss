; Inno Setup script for the OpenDownloader Windows installer.
; Build: iscc /DAppVersion=0.1.5 packaging\windows\opendownloader.iss
; Expects target\release\opendownloader.exe; writes dist\OpenDownloader-<version>-windows-x64-setup.exe.

#ifndef AppVersion
  #define AppVersion "0.1.5"
#endif

[Setup]
AppId={{6F1C2A43-8E0B-4C55-9B1E-0D9E5A7C2B31}
AppName=OpenDownloader
AppVersion={#AppVersion}
AppVerName=OpenDownloader {#AppVersion}
AppPublisher=OpenDownloader contributors
AppPublisherURL=https://github.com/marondayo/opendownloader
AppSupportURL=https://github.com/marondayo/opendownloader/issues
AppUpdatesURL=https://github.com/marondayo/opendownloader/releases
; Per-user install: no administrator rights needed.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\OpenDownloader
DefaultGroupName=OpenDownloader
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=OpenDownloader-{#AppVersion}-windows-x64-setup
SetupIconFile=..\icons\opendownloader.ico
UninstallDisplayIcon={app}\opendownloader.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\opendownloader.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\OpenDownloader"; Filename: "{app}\opendownloader.exe"
Name: "{autodesktop}\OpenDownloader"; Filename: "{app}\opendownloader.exe"; Tasks: desktopicon

[Registry]
; The app writes this key at startup (so the portable zip gets notifications too); remove it on uninstall.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\io.github.opendownloader"; Flags: uninsdeletekey dontcreatekey

[Run]
Filename: "{app}\opendownloader.exe"; Description: "{cm:LaunchProgram,OpenDownloader}"; Flags: nowait postinstall skipifsilent
; In-app updates run Setup with /SILENT; "Restart now" adds /relaunch=1 to start the new version afterwards.
Filename: "{app}\opendownloader.exe"; Flags: nowait runasoriginaluser; Check: RelaunchRequested

; Settings, history and the yt-dlp/FFmpeg/Deno copies live in %APPDATA%\OpenDownloader
; and are kept on uninstall so a reinstall picks up where you left off.

; [Code] is Pascal (comments there are // or { }), so it stays the last section.
[Code]
function RelaunchRequested: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:relaunch|0}') = '1');
end;
