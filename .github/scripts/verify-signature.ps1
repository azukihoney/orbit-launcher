param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [switch]$MetadataOnly,
    [string]$PythonExecutable = 'python'
)
$ErrorActionPreference = 'Stop'
$binaryPath = (Resolve-Path -LiteralPath $Binary).Path
$expectedVersion = (& $PythonExecutable -c "import tomllib; print(tomllib.load(open('Cargo.toml', 'rb'))['package']['version'])").Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot read Cargo version' }
$metadata = (Get-Item -LiteralPath $binaryPath).VersionInfo
$expected = @{
    ProductName = 'Orbit Launcher'
    FileDescription = 'Orbit Launcher'
    OriginalFilename = 'orbit-launcher.exe'
    InternalName = 'orbit-launcher.exe'
    CompanyName = 'azukihoney'
    LegalCopyright = 'azukihoney'
    ProductVersion = $expectedVersion
    FileVersion = $expectedVersion
}
foreach ($field in $expected.Keys) {
    if ($metadata.$field -cne $expected[$field]) { throw "Invalid VERSIONINFO field: $field" }
}
if ($MetadataOnly) { return }

$kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
$signtool = Get-ChildItem -LiteralPath $kits -Directory |
    Sort-Object Name -Descending |
    ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
    Where-Object { Test-Path -LiteralPath $_ } |
    Select-Object -First 1
if (-not $signtool) { throw 'Windows SDK x64 signtool.exe is required' }
& $signtool verify /pa /all /v /tw $binaryPath
if ($LASTEXITCODE -ne 0) { throw 'Authenticode verification failed or reported a warning' }
$signature = Get-AuthenticodeSignature -LiteralPath $binaryPath
if ($signature.Status -ne 'Valid') { throw 'Signature status must be Valid' }
if (-not $signature.TimeStamperCertificate) { throw 'A timestamp is required' }
if ($signature.SignerCertificate.PublicKey.Oid.Value -ne '1.2.840.113549.1.1.1') {
    throw 'An RSA signing certificate is required'
}
$certificate = $signature.SignerCertificate
$summary = @(
    'Authenticode verified; RSA signing key and timestamp present.'
    "Signer: $($certificate.Subject)"
    "Issuer: $($certificate.Issuer)"
    "Valid until: $($certificate.NotAfter.ToUniversalTime().ToString('o'))"
    "Timestamp signer: $($signature.TimeStamperCertificate.Subject)"
)
$summary | Write-Output
if ($env:GITHUB_STEP_SUMMARY) { $summary | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Encoding UTF8 }
