@echo off
echo ==========================================
echo  Building Zephyr (Release)
echo ==========================================

cargo build --release
if %errorlevel% neq 0 (
    echo [ERROR] Build failed with exit code %errorlevel%
    exit /b %errorlevel%
)

if not exist bin mkdir bin
copy /y target\release\zephyr.exe bin\zephyr.exe > nul
if %errorlevel% neq 0 (
    echo [ERROR] Failed to copy binary to bin\zephyr.exe
    exit /b %errorlevel%
)

echo.
echo [SUCCESS] Release binary successfully updated in bin\zephyr.exe
