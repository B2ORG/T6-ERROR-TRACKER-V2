$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (Test-Path -LiteralPath $cargoBin) {
    $env:PATH = "$cargoBin;$env:PATH"
}

$sysroot = & rustc --print sysroot
if ($LASTEXITCODE -ne 0) { throw 'A Windows Rust toolchain is required.' }
$hostLine = & rustc -vV | Select-String '^host: '
$rustHost = ($hostLine.ToString() -replace '^host: ', '').Trim()
if ($rustHost -eq 'x86_64-pc-windows-gnu') {
    $bundledTools = Join-Path $sysroot "lib\rustlib\$rustHost\bin\self-contained"
    $bundledGcc = Join-Path $bundledTools 'x86_64-w64-mingw32-gcc.exe'
    if (Test-Path -LiteralPath $bundledGcc) {
        $env:PATH = "$bundledTools;$env:PATH"
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $bundledGcc
    }
}

Push-Location $projectRoot
try {
    & cargo fmt --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed.' }
    & cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
    & cargo clippy --locked --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed.' }
    & cargo build --locked --release
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
} finally {
    Pop-Location
}
