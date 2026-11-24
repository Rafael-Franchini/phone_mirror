[Setup]
AppId={{8F9B3A1E-42C7-4D5A-9B10-7C8E2F4A1901}
AppName=PhoneMirror
AppVersion=1.0.0
AppVerName=PhoneMirror 1.0 (por Rafael Franchini)
AppPublisher=Rafael Franchini (@Rafael-Franchini)
AppPublisherURL=https://github.com/Rafael-Franchini
AppSupportURL=https://www.linkedin.com/in/rafael-franchini-37b0a21a4/
AppUpdatesURL=https://github.com/Rafael-Franchini
AppCopyright=Copyright (C) 2026 Rafael Franchini (github.com/Rafael-Franchini)
DefaultDirName={autopf}\PhoneMirror
DefaultGroupName=PhoneMirror
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=.
OutputBaseFilename=PhoneMirror_Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Messages]
brazilianportuguese.BeveledLabel=Desenvolvido por Rafael Franchini • github.com/Rafael-Franchini
brazilianportuguese.WelcomeLabel2=Este assistente instalará o [name/ver] no seu computador.%n%nDesenvolvido por: Rafael Franchini (R4LF)%nGitHub: https://github.com/Rafael-Franchini%nLinkedIn: https://www.linkedin.com/in/rafael-franchini-37b0a21a4/%n%nClique em Avançar para continuar.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "target\release\phone_mirror_gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "PhoneMirror.apk"; DestDir: "{app}"; Flags: ignoreversion
Source: "index.html"; DestDir: "{app}"; Flags: ignoreversion
Source: "politica-privacidade.html"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\PhoneMirror"; Filename: "{app}\phone_mirror_gui.exe"
Name: "{autoprograms}\PhoneMirror - Site de Apresentação"; Filename: "{app}\index.html"
Name: "{autoprograms}\PhoneMirror - Política de Privacidade (LGPD)"; Filename: "{app}\politica-privacidade.html"
Name: "{autoprograms}\PhoneMirror - GitHub (@Rafael-Franchini)"; Filename: "https://github.com/Rafael-Franchini"
Name: "{autodesktop}\PhoneMirror"; Filename: "{app}\phone_mirror_gui.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\phone_mirror_gui.exe"; Description: "{cm:LaunchProgram,PhoneMirror}"; Flags: nowait postinstall skipifsilent
