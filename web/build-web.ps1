# Builds the browser version of modplay (WebAssembly) into web\dist.
#
#   powershell -ExecutionPolicy Bypass -File D:\develop\modplay-rs\web\build-web.ps1
#   powershell -ExecutionPolicy Bypass -File D:\develop\modplay-rs\web\build-web.ps1 -Serve
#
#   -Serve   also start a local web server and open http://localhost:8000/ (needs Python)
#
# Needs rustup / cargo. The first run installs the wasm32 target and wasm-bindgen-cli.
param([switch]$Serve)

$ErrorActionPreference = 'Stop'
$web = Split-Path -Parent $MyInvocation.MyCommand.Path
$dist = Join-Path $web 'dist'
Set-Location $web

Write-Host '== rustup target'
rustup target add wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw 'rustup target add failed' }

Write-Host '== cargo build'
cargo build --release --target wasm32-unknown-unknown
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

# wasm-bindgen-cli must be exactly the version of the wasm-bindgen crate in Cargo.lock
$lock = Get-Content (Join-Path $web 'Cargo.lock')
$i = [Array]::IndexOf($lock, 'name = "wasm-bindgen"')
$want = ($lock[$i + 1] -replace '^version = "(.*)"$', '$1')
$have = ''
if (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) { $have = ((wasm-bindgen --version) -split ' ')[1] }
if ($have -ne $want) {
  Write-Host "== cargo install wasm-bindgen-cli $want"
  cargo install wasm-bindgen-cli --version $want --locked
  if ($LASTEXITCODE -ne 0) { throw 'installing wasm-bindgen-cli failed' }
}

Write-Host '== wasm-bindgen'
wasm-bindgen --target web --no-typescript --out-dir $dist (Join-Path $web 'target\wasm32-unknown-unknown\release\modplay_web.wasm')
if ($LASTEXITCODE -ne 0) { throw 'wasm-bindgen failed' }
Copy-Item (Join-Path $web 'index.html') $dist -Force

Write-Host ''
Write-Host "== done: $dist"
Get-ChildItem $dist | ForEach-Object { '{0,10:N0}  {1}' -f $_.Length, $_.Name }

if ($Serve) {
  # pages with WebAssembly modules cannot be opened as file://, they need a web server
  Start-Process 'http://localhost:8000/'
  python -m http.server 8000 --directory $dist
}
