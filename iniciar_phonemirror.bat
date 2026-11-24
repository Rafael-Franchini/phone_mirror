@echo off
title PhoneMirror Studio - Gerenciador de Conexoes
cd /d "%~dp0"

echo ==============================================================
echo             PHONEMIRROR STUDIO - INTERFACE GRAFICA
echo ==============================================================
echo.
echo Iniciando a Interface Grafica do PhoneMirror...
echo.

if exist "target\release\phone_mirror_gui.exe" (
    start "" "target\release\phone_mirror_gui.exe"
    exit /b 0
)

if exist "target\debug\phone_mirror_gui.exe" (
    start "" "target\debug\phone_mirror_gui.exe"
    exit /b 0
)

cargo run --bin phone_mirror_gui --release

if %errorlevel% neq 0 (
    cargo run --bin phone_mirror_gui
)
