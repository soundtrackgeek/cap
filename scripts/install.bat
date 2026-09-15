@echo off
setlocal DisableDelayedExpansion
title Install cap

echo Installing cap for your Windows user account...
echo.
if not exist "%~dp0install.ps1" goto missing_package
if not exist "%~dp0bin\cap.exe" goto missing_package
if not exist "%~dp0checksums.sha256" goto missing_package
if not exist "%~dp0manifest.json" goto missing_package

"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" -SourcePath "%~dp0bin\cap.exe" -PackageRoot "%~dp0." -PromptForReplace %*
set "installExitCode=%errorlevel%"
echo.
if "%installExitCode%"=="2" goto done
if not "%installExitCode%"=="0" goto install_failed
echo Installation complete. Open a new terminal and run: cap --help
goto done

:missing_package
echo Installation could not start. Extract all files from the release ZIP first,
echo then double-click install.bat inside the extracted folder.
set "installExitCode=1"
goto done

:install_failed
echo Installation failed. Review the error above, then try again.

:done
echo.
pause
exit /b %installExitCode%
