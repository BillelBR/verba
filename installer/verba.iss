; Inno Setup 6.3 or later script for Verba.
;
; Build it with scripts\build-installer.ps1 (which passes the version), or by hand:
;     ISCC.exe /DAppVersion=0.11.0 installer\verba.iss
; Output: dist\Verba-Setup-<version>.exe
;
; Per-user by default (no administrator rights needed); the "dialog" override lets the user choose
; to install for all users instead.

#ifndef AppVersion
  #define AppVersion "0.11.0"
#endif

[Setup]
; fixed forever: it is how Windows (and the next version's setup) recognises this program
AppId={{E3A22341-710A-4675-8C89-D3E0E0D70971}
AppName=Verba
AppVersion={#AppVersion}
AppVerName=Verba {#AppVersion}
AppPublisher=Verba contributors
AppPublisherURL=https://github.com/BillelBR/verba
AppSupportURL=https://github.com/BillelBR/verba/issues
AppUpdatesURL=https://github.com/BillelBR/verba/releases
AppCopyright=Verba contributors
; Windows 10 version 1809 or later, 64-bit only
MinVersion=10.0.17763
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\Verba
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir=..\dist
OutputBaseFilename=Verba-Setup-{#AppVersion}
SetupIconFile=..\crates\verba-app\assets\verba.ico
UninstallDisplayIcon={app}\verba.exe
UninstallDisplayName=Verba
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; the page that tells where the word lists come from and under which license
InfoBeforeFile=word-lists-notice.txt
; Verba is closed by the [Code] below (a graceful quit first, then taskkill)
CloseApplications=no
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[CustomMessages]
english.TaskAutostart=Start Verba when Windows starts
english.TaskSentences=Sentence translation, Arabic and English (about 330 MB on disk)
english.TaskGroupOptions=Options:
english.LaunchVerba=Launch Verba
english.AskDeleteSettings=Also delete your Verba settings (the theme, languages and program list you chose)?
french.TaskAutostart=Démarrer Verba au démarrage de Windows
french.TaskSentences=Traduction de phrases, arabe et anglais (environ 330 Mo sur le disque)
french.TaskGroupOptions=Options :
french.LaunchVerba=Lancer Verba
french.AskDeleteSettings=Supprimer aussi vos réglages Verba (thème, langues et liste de programmes choisis) ?

[Tasks]
Name: "autostart"; Description: "{cm:TaskAutostart}"; GroupDescription: "{cm:TaskGroupOptions}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
#ifdef WithSentences
; ticked by default; unticking it leaves out the translator program and the two models (the installer is built with them when they exist)
Name: "sentences"; Description: "{cm:TaskSentences}"; GroupDescription: "{cm:TaskGroupOptions}"
#endif

[Files]
Source: "..\target\release\verba.exe"; DestDir: "{app}"; Flags: ignoreversion
; the word lists: separate files with their own license (CC-BY-SA-4.0), see word-lists-notice.txt
Source: "..\data\words\*.txt"; DestDir: "{app}\data\words"; Flags: ignoreversion
Source: "word-lists-notice.txt"; DestDir: "{app}"; DestName: "WORD-LISTS-LICENSE.txt"; Flags: ignoreversion
#ifdef WithSentences
; sentence translation: a program of its own (CTranslate2 and oneDNN, MIT and Apache-2.0) and one OPUS-MT model per direction (Apache-2.0),
; see THIRD-PARTY.md. vcomp140.dll is the Visual C++ OpenMP runtime, a redistributable file of Microsoft.
Source: "..\target\mt\release\verba-translate.exe"; DestDir: "{app}"; Flags: ignoreversion; Tasks: sentences
Source: "..\target\mt\release\vcomp140.dll"; DestDir: "{app}"; Flags: ignoreversion; Tasks: sentences
Source: "..\models\ar-en\*"; DestDir: "{app}\models\ar-en"; Flags: ignoreversion; Tasks: sentences
Source: "..\models\en-ar\*"; DestDir: "{app}\models\en-ar"; Flags: ignoreversion; Tasks: sentences
#endif
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\COMMERCIAL.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\DATA.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Verba"; Filename: "{app}\verba.exe"
Name: "{autodesktop}\Verba"; Filename: "{app}\verba.exe"; Tasks: desktopicon

[Registry]
; the same per-user value the program itself writes from its tray menu ("Start with Windows")
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Verba"; \
  ValueData: """{app}\verba.exe"" --startup"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\verba.exe"; Description: "{cm:LaunchVerba}"; Flags: nowait postinstall skipifsilent

[Code]
const
  WM_COMMAND = $0111;
  CMD_QUIT = 1002; { what the tray menu's "Quit Verba" sends to the program's hidden window }
  RUN_KEY = 'Software\Microsoft\Windows\CurrentVersion\Run';

{ Asks a running Verba to quit, waits up to 5 seconds, and only then forces it closed. }
procedure CloseVerba();
var
  wnd: HWND;
  i, code: Integer;
begin
  wnd := FindWindowByClassName('VerbaController');
  if wnd <> 0 then
  begin
    PostMessage(wnd, WM_COMMAND, CMD_QUIT, 0);
    for i := 1 to 25 do
    begin
      Sleep(200);
      if FindWindowByClassName('VerbaController') = 0 then
        Break;
    end;
  end;
  if FindWindowByClassName('VerbaController') <> 0 then
    Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM verba.exe', '', SW_HIDE, ewWaitUntilTerminated, code);
  { the sentence translator may be running (it ends by itself after ten minutes): its files must not be in use }
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM verba-translate.exe', '', SW_HIDE, ewWaitUntilTerminated, code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseVerba();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  CloseVerba();
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  data: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    { the start-with-Windows value, also when the user switched it on later from the tray menu;
      only when it points into this installation }
    if RegQueryStringValue(HKCU, RUN_KEY, 'Verba', data) then
      if Pos(Lowercase(ExpandConstant('{app}')), Lowercase(data)) > 0 then
        RegDeleteValue(HKCU, RUN_KEY, 'Verba');

    { the settings stay unless the user says yes (a silent uninstall never deletes them) }
    if not UninstallSilent then
      if MsgBox(ExpandConstant('{cm:AskDeleteSettings}'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(ExpandConstant('{userappdata}\Verba'), True, True, True);
  end;
end;
