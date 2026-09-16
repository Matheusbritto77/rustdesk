# Powershell Script to Sign Remora.exe and prevent Windows Defender / SmartScreen false positives
param (
    [string]$ExePath = "..\web\download\Remora.exe",
    [string]$CertSubject = "CN=Remora Software Solutions, O=Remora Desk, C=BR"
)

Write-Host "[+] Checking Code Signing Certificate..." -ForegroundColor Cyan

# Find or Create Self-Signed Code Signing Certificate
$cert = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object { $_.Subject -match "Remora" } | Select-Object -First 1

if (-not $cert) {
    Write-Host "[+] Creating Self-Signed Code Signing Certificate for Remora..." -ForegroundColor Yellow
    $cert = New-SelfSignedCertificate -Type CodeSigningCert `
        -Subject $CertSubject `
        -CertStoreLocation Cert:\CurrentUser\My `
        -NotAfter (Get-Date).AddYears(5)
    
    # Export to Trusted Root Authorities to trust locally
    $rootStore = New-Object System.Security.Cryptography.X509Certificates.X509Store("Root", "CurrentUser")
    $rootStore.Open("ReadWrite")
    $rootStore.Add($cert)
    $rootStore.Close()
    Write-Host "[+] Certificate created and installed in Trusted Root Authorities." -ForegroundColor Green
} else {
    Write-Host "[+] Found existing Remora Code Signing Certificate." -ForegroundColor Green
}

if (Test-Path $ExePath) {
    Write-Host "[+] Signing $ExePath..." -ForegroundColor Cyan
    Set-AuthenticodeSignature -FilePath $ExePath -Certificate $cert -TimestampServer "http://timestamp.digicert.com"
    Write-Host "[SUCCESS] $ExePath has been digitally signed!" -ForegroundColor Green
} else {
    Write-Host "[!] Target file $ExePath not found!" -ForegroundColor Red
}
