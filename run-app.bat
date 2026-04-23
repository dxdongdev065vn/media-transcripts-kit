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
echo My Media Kit - Run dev app
echo ========================================
echo.

where npm >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Khong tim thay npm trong PATH.
  echo Hay chay install.bat sau khi cai Node.js 20+.
  exit /b 1
)

if not exist "node_modules" (
  echo [INFO] Chua thay node_modules. Dang chay install truoc...
  call "%SCRIPT_DIR%install.bat"
  if errorlevel 1 (
    echo.
    echo [ERROR] Khong the tiep tuc vi cai dat that bai.
    exit /b 1
  )
)

echo.
echo Dang khoi dong app...
call npm run dev
set "EXIT_CODE=%errorlevel%"
popd
exit /b %EXIT_CODE%
