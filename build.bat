@echo off
echo ==========================================
echo  Building MySQL TurboLoad Enterprise (Release)
echo ==========================================

cargo build --release
if %errorlevel% neq 0 (
    echo [ERROR] Build failed with exit code %errorlevel%
    exit /b %errorlevel%
)

if not exist bin mkdir bin
copy /y target\release\mysql-turboload.exe bin\mysql-turboload.exe > nul
if %errorlevel% neq 0 (
    echo [ERROR] Failed to copy binary to bin\mysql-turboload.exe
    exit /b %errorlevel%
)

echo.
echo [SUCCESS] Release binary successfully updated in bin\mysql-turboload.exe
