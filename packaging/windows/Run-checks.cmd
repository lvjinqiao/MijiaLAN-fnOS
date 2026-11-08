@echo off
setlocal
for %%T in ("%~dp0checks\*.exe") do (
    echo Running %%~nxT
    "%%~fT" --test-threads=1
    if errorlevel 1 exit /b 1
)
echo All backend checks passed.
exit /b 0
