<#
.SYNOPSIS
    Downloads EmbeddingGemma 2 Q8_0 GGUF models for Nori.
.DESCRIPTION
    Fetches the text model and vision/audio projector from Hugging Face:
    ggml-org/embeddinggemma-2-GGUF (commit bfcd298762cc34d0357ece5ebdd31791a3a374d8).
    Checks local Hugging Face hub cache first, resumes partial downloads, and validates
    SHA-256 checksums before moving into the target directory.
.PARAMETER To
    Target directory (default: $env:LOCALAPPDATA\Nori\Models).
#>

[CmdletBinding()]
param (
    [Parameter(Mandatory=$false)]
    [string]$To
)

$ErrorActionPreference = "Stop"

$Repo = "ggml-org/embeddinggemma-2-GGUF"
$Rev = "bfcd298762cc34d0357ece5ebdd31791a3a374d8"
$BaseUrl = "https://huggingface.co/$Repo/resolve/$Rev"

$TextFile = @{
    Name = "embeddinggemma-2-Q8_0.gguf"
    Bytes = 309855456
    Sha256 = "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135"
}

$MediaFile = @{
    Name = "mmproj-embeddinggemma-2-Q8_0.gguf"
    Bytes = 554821024
    Sha256 = "c4a8a52691ecef40618438928bdf9e68379b854e24166f292592353db0aab64f"
}

if (-not $To) {
    if ($env:LOCALAPPDATA) {
        $To = Join-Path $env:LOCALAPPDATA "Nori\Models"
    } else {
        $To = Join-Path $HOME ".local\share\nori\Models"
    }
}

if (-not (Test-Path $To)) {
    New-Item -ItemType Directory -Path $To -Force | Out-Null
}

Write-Host "==> Nori Models destination: $To" -ForegroundColor Cyan

# Hugging Face local cache directory check
$HfCache = if ($env:HF_HUB_CACHE) { $env:HF_HUB_CACHE } `
           elseif ($env:HF_HOME) { Join-Path $env:HF_HOME "hub" } `
           else { Join-Path $HOME ".cache\huggingface\hub" }

$RepoDir = "models--" + ($Repo -replace "/", "--")
$SnapshotDir = Join-Path (Join-Path $HfCache $RepoDir) "snapshots\$Rev"

function Get-FileSha256 {
    param ([string]$Path)
    $hash = Get-FileHash -Path $Path -Algorithm SHA256
    return $hash.Hash.ToLower()
}

function Download-ModelFile {
    param (
        [hashtable]$FileDef
    )

    $name = $FileDef.Name
    $expectedBytes = [int64]$FileDef.Bytes
    $expectedSha = $FileDef.Sha256
    $dest = Join-Path $To $name
    $partial = Join-Path $To "$name.partial"
    $url = "$BaseUrl/$name"

    # 1. Existing verified file
    if (Test-Path $dest) {
        $item = Get-Item $dest
        if ($item.Length -eq $expectedBytes) {
            Write-Host "--> $name exists with valid size. Verifying SHA-256..." -ForegroundColor Yellow
            $sha = Get-FileSha256 -Path $dest
            if ($sha -eq $expectedSha) {
                Write-Host "--> [OK] $name verified." -ForegroundColor Green
                return
            } else {
                Write-Warning "SHA-256 mismatch for existing $name. Re-downloading..."
                Remove-Item $dest -Force
            }
        }
    }

    # 2. Check Hugging Face Hub cache
    $cached = Join-Path $SnapshotDir $name
    if (Test-Path $cached) {
        $cachedItem = Get-Item $cached
        if ($cachedItem.Length -eq $expectedBytes) {
            Write-Host "--> Found $name in Hugging Face cache ($cached). Verifying..." -ForegroundColor Yellow
            $cachedSha = Get-FileSha256 -Path $cached
            if ($cachedSha -eq $expectedSha) {
                Write-Host "--> Copying from cache to $dest..." -ForegroundColor Cyan
                Copy-Item -Path $cached -Destination $dest -Force
                Write-Host "--> [OK] $name copied and verified." -ForegroundColor Green
                return
            }
        }
    }

    # 3. Download using curl (resumable) if available, otherwise BITS or HttpClient
    $sizeMB = [math]::Round($expectedBytes / 1MB, 1)
    Write-Host "--> Downloading $name (~$sizeMB MB)..." -ForegroundColor Cyan

    $curlCmd = Get-Command "curl.exe" -ErrorAction SilentlyContinue
    if ($curlCmd) {
        & curl.exe -L -C - --progress-bar -o "$partial" "$url"
    } else {
        # Fallback to .NET HttpClient with chunked download
        $client = [System.Net.Http.HttpClient]::new()
        $response = $client.GetAsync($url, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
        $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $fileStream = [System.IO.File]::Create($partial)
        $buffer = [byte[]]::new(81920)
        $totalRead = 0
        while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $fileStream.Write($buffer, 0, $read)
            $totalRead += $read
        }
        $fileStream.Dispose()
        $stream.Dispose()
        $client.Dispose()
    }

    Write-Host "--> Verifying SHA-256 checksum for $name..." -ForegroundColor Yellow
    $downloadedSha = Get-FileSha256 -Path $partial
    if ($downloadedSha -ne $expectedSha) {
        Remove-Item $partial -Force -ErrorAction SilentlyContinue
        throw "SHA-256 verification failed for $name! Expected: $expectedSha, got: $downloadedSha"
    }

    Move-Item -Path $partial -Destination $dest -Force
    Write-Host "--> [OK] $name successfully downloaded and verified." -ForegroundColor Green
}

Write-Host ""
Write-Host "=== Step 1/2: Text Model ($($TextFile.Name)) ===" -ForegroundColor White
Download-ModelFile -FileDef $TextFile

Write-Host ""
Write-Host "=== Step 2/2: Multimodal Projector ($($MediaFile.Name)) ===" -ForegroundColor White
Download-ModelFile -FileDef $MediaFile

Write-Host ""
Write-Host "All Nori models are installed and ready at: $To" -ForegroundColor Green
