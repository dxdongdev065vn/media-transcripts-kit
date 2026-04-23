@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
if /I "%SCRIPT_DIR:~0,8%"=="\\?\UNC\" set "SCRIPT_DIR=\\%SCRIPT_DIR:~8%"

pushd "%SCRIPT_DIR%" >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Khong the truy cap thu muc app:
  echo %SCRIPT_DIR%
  exit /b 1
)

echo ========================================
echo My Media Kit - Windows install
echo ========================================
echo.

where node >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Khong tim thay Node.js trong PATH.
  echo Cai Node.js 20+ truoc: https://nodejs.org/
  exit /b 1
)

where npm >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Khong tim thay npm trong PATH.
  echo Vui long cai lai Node.js 20+ kem npm.
  exit /b 1
)

where rustc >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Khong tim thay rustc trong PATH.
  echo Tauri can Rust 1.80+ de build backend.
  echo Cai tai: https://rustup.rs/
  exit /b 1
)

rustc --version

where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Khong tim thay cargo trong PATH.
  echo Cai Rust qua rustup roi mo lai terminal de cap nhat PATH.
  echo Cai tai: https://rustup.rs/
  exit /b 1
)

cargo --version

where ffmpeg >nul 2>nul
if errorlevel 1 (
  echo [WARN] Khong tim thay ffmpeg trong PATH.
  echo App dev mode nen co ffmpeg va ffprobe trong PATH.
)

where ffprobe >nul 2>nul
if errorlevel 1 (
  echo [WARN] Khong tim thay ffprobe trong PATH.
)

echo.
echo Dang cai npm dependencies...
call npm install
if errorlevel 1 (
  echo.
  echo [ERROR] npm install that bai.
  exit /b 1
)

echo.
echo Cai dat hoan tat.
echo Chay app bang file run-app.bat
popd
exit /b 0
