@echo off
chcp 65001 >nul
echo =========================================
echo ETW Profiler Symbol Debug Script
echo =========================================
echo.

:: Set environment variables for debugging
set RUST_LOG=info
set RUST_BACKTRACE=1

:: Clean up previous test files
if exist symbols_debug.csv del symbols_debug.csv
if exist debug_output.txt del debug_output.txt

echo Building project...
cargo build --release 2>&1 | findstr /V "Compiling Finished Running"

if %ERRORLEVEL% NEQ 0 (
    echo Build failed!
    exit /b 1
)

echo.
echo =========================================
echo Running profiler with detailed logging...
echo =========================================
echo.

:: Run the profiler with verbose logging
target\release\etw-profiler.exe profile --duration 2 --output symbols_debug.csv --verbose 2>&1 | tee debug_output.txt

echo.
echo =========================================
echo Analyzing output...
echo =========================================
echo.

:: Check for symbol-related messages
echo === Symbol Loading Messages ===
findstr /I "SYMBOL_LOAD" debug_output.txt 2>nul || echo No symbol load messages found

echo.
echo === Symbol Resolution Messages ===
findstr /I "SYMBOL_RESOLVE" debug_output.txt 2>nul || echo No symbol resolve messages found

echo.
echo === Module Loading Messages ===
findstr /I "MODULE_LOAD" debug_output.txt 2>nul || echo No module load messages found

echo.
echo === Sample Resolution Messages ===
findstr /I "SAMPLE_RESOLVE" debug_output.txt 2>nul || echo No sample resolve messages found

echo.
echo === PDB File Messages ===
findstr /I "PDB" debug_output.txt 2>nul || echo No PDB messages found

echo.
echo === Error Messages ===
findstr /I "error Error ERROR" debug_output.txt 2>nul || echo No error messages found

echo.
echo =========================================
echo CSV Output Summary
echo =========================================
if exist symbols_debug.csv (
    echo Total samples in CSV:
    findstr /V "#" symbols_debug.csv | find /c ","
    
    echo.
    echo Checking for resolved symbols (not starting with 0x):
    findstr /V "#" symbols_debug.csv | findstr /V "0x00000000" | find /c ","
) else (
    echo No output CSV file found!
)

echo.
echo =========================================
echo Debug log saved to: debug_output.txt
echo CSV output saved to: symbols_debug.csv
echo =========================================

pause
