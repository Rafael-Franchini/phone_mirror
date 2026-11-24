@echo off
title PhoneMirror Studio - Launcher
cd /d "%~dp0"

echo ==============================================================
echo             PHONEMIRROR - INTERFACE GRAFICA (PC)
echo ==============================================================
echo.
echo Iniciando a Interface Grafica do PhoneMirror...
echo.

cargo run --bin phone_mirror_gui --release

if %errorlevel% neq 0 (
    echo.
    echo Tentando executar em modo debug...
    cargo run --bin phone_mirror_gui
)

pause
