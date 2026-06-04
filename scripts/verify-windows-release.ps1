param(
  [string]$Repo = "oshtz/dybur",
  [string]$AssetName = "dybur-windows-x64.exe",
  [int]$MinSizeMB = 15,
  [string]$OutputDir = "",
  [string]$ExpectedSha256 = "",
  [switch]$RequireSignature,
  [switch]$Json
)

$ErrorActionPreference = "Stop"

function New-DirectoryIfMissing {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
  }
}

if (-not $OutputDir) {
  $OutputDir = Join-Path ([System.IO.Path]::GetTempPath()) "dybur-release-smoke"
}

New-DirectoryIfMissing -Path $OutputDir

$downloadUrl = "https://github.com/$Repo/releases/latest/download/$AssetName"
$downloadPath = Join-Path $OutputDir $AssetName

Remove-Item -LiteralPath $downloadPath -Force -ErrorAction SilentlyContinue
Invoke-WebRequest -Uri $downloadUrl -OutFile $downloadPath -MaximumRedirection 5 -Headers @{
  "User-Agent" = "dybur-windows-release-verifier"
}

$file = Get-Item -LiteralPath $downloadPath
$hash = (Get-FileHash -LiteralPath $downloadPath -Algorithm SHA256).Hash.ToLowerInvariant()
$signature = Get-AuthenticodeSignature -LiteralPath $downloadPath

$issues = New-Object System.Collections.Generic.List[string]
$warnings = New-Object System.Collections.Generic.List[string]

if ($file.Length -lt ($MinSizeMB * 1MB)) {
  $issues.Add("$AssetName is unexpectedly small: $($file.Length) bytes")
}

if ($ExpectedSha256 -and ($hash -ne $ExpectedSha256.ToLowerInvariant())) {
  $issues.Add("$AssetName SHA-256 mismatch: expected $ExpectedSha256, got $hash")
}

if ($signature.Status -ne "Valid") {
  $message = "$AssetName Authenticode status is $($signature.Status)"
  if ($RequireSignature) {
    $issues.Add($message)
  } else {
    $warnings.Add($message)
  }
}

$summary = [pscustomobject]@{
  ok = $issues.Count -eq 0
  repo = $Repo
  assetName = $AssetName
  downloadUrl = $downloadUrl
  downloadPath = $downloadPath
  sizeBytes = $file.Length
  sizeMB = [math]::Round($file.Length / 1MB, 1)
  sha256 = $hash
  signatureStatus = [string]$signature.Status
  signatureSigner = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }
  warnings = @($warnings)
  issues = @($issues)
}

if ($Json) {
  $summary | ConvertTo-Json -Depth 4
} else {
  Write-Host "Windows release asset: $($summary.assetName)"
  Write-Host "Downloaded: $($summary.downloadPath)"
  Write-Host "Size: $($summary.sizeMB) MB"
  Write-Host "SHA-256: $($summary.sha256)"
  Write-Host "Authenticode: $($summary.signatureStatus)"

  if ($summary.signatureSigner) {
    Write-Host "Signer: $($summary.signatureSigner)"
  }

  if ($warnings.Count -gt 0) {
    Write-Host ""
    Write-Host "Warnings:"
    foreach ($warning in $warnings) {
      Write-Host "  - $warning"
    }
  }

  if ($issues.Count -gt 0) {
    Write-Host ""
    Write-Host "Issues:"
    foreach ($issue in $issues) {
      Write-Host "  - $issue"
    }
  }
}

if ($issues.Count -gt 0) {
  exit 1
}
