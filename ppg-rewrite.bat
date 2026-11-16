@echo off
rem People Playground rewrite: builds and starts it. You need your own People Playground (Steam); it is found by itself,
rem or drag its folder onto this file, or pick it when asked. From Steam: see the README (Steam's %command% is handled).
setlocal
title People Playground rewrite
rem (shift below moves %0 too: the folder is kept first.)
set "ROOT=%~dp0"
cd /d "%ROOT%"
rem Steam starts this without your PATH: rustup's cargo first.
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

rem The game folder: a folder dropped on this file, or the game's exe (Steam's %command% passes
rem "...\People Playground.exe"; it is never started). Everything else is ignored.
set "GAMEDIR="
:args
if "%~1"=="" goto args_done
if exist "%~1\People Playground.exe" set "GAMEDIR=%~f1"
if /i "%~nx1"=="People Playground.exe" if exist "%~1" set "GAMEDIR=%~dp1"
shift
goto args
:args_done
rem (Skipped when unset: cmd cannot even parse %GAMEDIR:~-1% then.)
if not defined GAMEDIR goto gamedir_done
if "%GAMEDIR:~-1%"=="\" set "GAMEDIR=%GAMEDIR:~0,-1%"
:gamedir_done

where cargo >nul 2>&1
if errorlevel 1 call :install || exit /b 1
echo Building (the first time takes several minutes)...
cargo build --release --manifest-path "%ROOT%Cargo.toml"
if errorlevel 1 (echo Build failed. & pause & exit /b 1)
if not defined WGPU_BACKEND set "WGPU_BACKEND=dx12"
rem What it writes as the game would beside itself (Logs, Contraptions, Mods, stats) goes to your user folder, not here.
if not exist "%LOCALAPPDATA%\ppg-rewrite" mkdir "%LOCALAPPDATA%\ppg-rewrite"
cd /d "%LOCALAPPDATA%\ppg-rewrite"
if defined GAMEDIR ("%ROOT%target\release\people-playground-bevy.exe" --game-dir "%GAMEDIR%") else ("%ROOT%target\release\people-playground-bevy.exe")
if errorlevel 1 (echo. & echo The rewrite stopped: the lines above say why. & pause)
exit /b 0

:install
echo The rewrite is built from source: it needs Rust and Microsoft's C++ build tools (free; a few GB).
choice /m "Install them now (winget)"
if errorlevel 2 exit /b 1
winget install -e --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install -e --id Rustlang.Rustup
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>&1
if errorlevel 1 (echo Rust didn't install. Get it from https://rustup.rs, then run this again. & pause & exit /b 1)
exit /b 0
