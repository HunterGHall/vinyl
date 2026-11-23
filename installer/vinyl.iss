; Vinyl installer. Build with installer\build.ps1 (needs Inno Setup 6).
; Installs per user (no admin prompt) into %LOCALAPPDATA%\Programs\Vinyl. Songs, playlists and
; settings live in %LOCALAPPDATA%\Vinyl and are left alone by updates and by the uninstaller.

#ifndef AppVersion
  #define AppVersion "0.0.1"
#endif

[Setup]
AppId={{6C2B7A0E-5B1D-4C55-9E0A-7A3D1F0B8E21}
AppName=Vinyl
AppVersion={#AppVersion}
AppVerName=Vinyl {#AppVersion}
AppPublisher=Hunter Hall
AppPublisherURL=https://github.com/HunterGHall/vinyl
AppSupportURL=https://github.com/HunterGHall/vinyl/issues
AppUpdatesURL=https://github.com/HunterGHall/vinyl/releases
DefaultDirName={autopf}\Vinyl
DefaultGroupName=Vinyl
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\dist
OutputBaseFilename=vinyl-setup
SetupIconFile=vinyl.ico
UninstallDisplayIcon={app}\vinyl.ico
UninstallDisplayName=Vinyl
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
CloseApplicationsFilter=vinyl.exe
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Vinyl setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "tools"; Description: "Install yt-dlp and ffmpeg (needed to add music) with winget"; GroupDescription: "Adding music:"; Check: HasWinget

[Files]
Source: "..\target\release\vinyl.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "vinyl.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; AppUserModelID must match APP_ID in src/main.rs: it is what lets Windows group the running window
; under the pinned taskbar icon instead of showing a second one.
Name: "{autoprograms}\Vinyl"; Filename: "{app}\vinyl.exe"; IconFilename: "{app}\vinyl.ico"; AppUserModelID: "HunterGHall.Vinyl"
Name: "{autodesktop}\Vinyl"; Filename: "{app}\vinyl.exe"; IconFilename: "{app}\vinyl.ico"; AppUserModelID: "HunterGHall.Vinyl"; Tasks: desktopicon

[Run]
Filename: "winget"; Parameters: "install --id yt-dlp.yt-dlp -e --silent --accept-package-agreements --accept-source-agreements"; StatusMsg: "Installing yt-dlp..."; Flags: runhidden waituntilterminated; Tasks: tools
Filename: "winget"; Parameters: "install --id Gyan.FFmpeg -e --silent --accept-package-agreements --accept-source-agreements"; StatusMsg: "Installing ffmpeg..."; Flags: runhidden waituntilterminated; Tasks: tools
; Normal install: offer to launch. Silent install (an in-app update): relaunch automatically.
Filename: "{app}\vinyl.exe"; Description: "Launch Vinyl"; Flags: nowait postinstall skipifsilent
Filename: "{app}\vinyl.exe"; Flags: nowait runasoriginaluser; Check: WizardSilent

[Code]
function HasWinget: Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C winget --version', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
end;

function WebViewInstalled: Boolean;
var
  V: String;
begin
  Result :=
    RegQueryStringValue(HKLM, 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', V) or
    RegQueryStringValue(HKCU, 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', V);
  Result := Result and (V <> '') and (V <> '0.0.0.0');
end;

function InitializeSetup: Boolean;
begin
  Result := True;
  if (not WebViewInstalled) and (not WizardSilent) then
    if MsgBox('Vinyl needs the Microsoft Edge WebView2 Runtime, which was not found on this PC. ' +
              'It is built into Windows 11 and most up-to-date Windows 10 installs.' + #13#10#13#10 +
              'Install Vinyl anyway? You can get the runtime from microsoft.com/edge/webview2.',
              mbConfirmation, MB_YESNO) = IDNO then
      Result := False;
end;
